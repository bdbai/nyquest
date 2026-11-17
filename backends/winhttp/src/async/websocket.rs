use std::borrow::Cow;
use std::ffi::c_void;
use std::mem::MaybeUninit;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll, Waker};

use futures_channel::oneshot;
use futures_core::Stream;
use futures_sink::Sink;
use nyquest_interface::websocket::{
    WebSocketError, WebSocketInfo, WebSocketMessage, WebSocketResult,
};
use windows_sys::Win32::Networking::WinHttp::*;

use super::context::{RequestContext, RequestState};
use super::state_fut::wait_for_state;
use crate::error::WinHttpError;
use crate::handle::ConnectionHandle;
use crate::request::{create_request, method_to_cwstr};
use crate::session::WinHttpSession;
use crate::url::ParsedUrl;
use crate::websocket::{
    build_websocket_headers, from_winhttp_error, prepare_websocket_url, query_negotiated_protocol,
    winhttp_error, WebSocketHandle, WEBSOCKET_RECV_BUFFER_SIZE,
};

enum SendState {
    Idle,
    InFlight,
    Done(Result<(), WinHttpError>),
}

struct SendInner {
    state: SendState,
    waker: Waker,
    buffer: Vec<u8>,
}

enum RecvState {
    Idle,
    InFlight,
    Done(Result<(usize, WINHTTP_WEB_SOCKET_BUFFER_TYPE), WinHttpError>),
}

struct RecvInner {
    state: RecvState,
    waker: Waker,
    buffer: Option<Box<MaybeUninit<[u8; WEBSOCKET_RECV_BUFFER_SIZE]>>>,
}

struct WsContext {
    send: Mutex<SendInner>,
    recv: Mutex<RecvInner>,
}

impl WsContext {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            send: Mutex::new(SendInner {
                state: SendState::Idle,
                waker: futures_task::noop_waker(),
                buffer: Vec::new(),
            }),
            recv: Mutex::new(RecvInner {
                state: RecvState::Idle,
                waker: futures_task::noop_waker(),
                buffer: None,
            }),
        })
    }

    fn complete_send(&self, res: Result<(), WinHttpError>) {
        let mut send = self.send.lock().unwrap();
        send.state = SendState::Done(res);
        send.waker.wake_by_ref();
    }

    fn complete_recv(&self, res: Result<(usize, WINHTTP_WEB_SOCKET_BUFFER_TYPE), WinHttpError>) {
        let mut recv = self.recv.lock().unwrap();
        recv.state = RecvState::Done(res);
        recv.waker.wake_by_ref();
    }
}

unsafe extern "system" fn ws_callback(
    _h_internet: *mut c_void,
    dw_context: usize,
    dw_internet_status: u32,
    lpv_status_information: *mut c_void,
    _dw_status_information_length: u32,
) {
    if dw_context == 0 {
        return;
    }

    if dw_internet_status == WINHTTP_CALLBACK_STATUS_HANDLE_CLOSING {
        let ctx = unsafe { Arc::<WsContext>::from_raw(dw_context as _) };
        drop(ctx);
        return;
    }

    let ctx = unsafe { &*(dw_context as *const WsContext) };
    match dw_internet_status {
        WINHTTP_CALLBACK_STATUS_WRITE_COMPLETE | WINHTTP_CALLBACK_STATUS_SHUTDOWN_COMPLETE => {
            ctx.complete_send(Ok(()));
        }
        WINHTTP_CALLBACK_STATUS_READ_COMPLETE => {
            if lpv_status_information.is_null() {
                ctx.complete_recv(Err(WinHttpError::from_code(
                    0,
                    "null status_info in WebSocket READ_COMPLETE",
                )));
            } else {
                let status =
                    unsafe { &*(lpv_status_information as *const WINHTTP_WEB_SOCKET_STATUS) };
                ctx.complete_recv(Ok((status.dwBytesTransferred as usize, status.eBufferType)));
            }
        }
        WINHTTP_CALLBACK_STATUS_REQUEST_ERROR => {
            if lpv_status_information.is_null() {
                let err = WinHttpError::from_code(0, "null status_info in WebSocket REQUEST_ERROR");
                ctx.complete_send(Err(err));
            } else {
                let res =
                    unsafe { &*(lpv_status_information as *const WINHTTP_WEB_SOCKET_ASYNC_RESULT) };
                let err = WinHttpError::from_code(res.AsyncResult.dwError, "async websocket error");
                if res.Operation == WINHTTP_WEB_SOCKET_RECEIVE_OPERATION {
                    ctx.complete_recv(Err(err));
                } else {
                    ctx.complete_send(Err(err));
                }
            }
        }
        _ => {}
    }
}

struct AsyncSocket {
    handle: WebSocketHandle,
    _connection: ConnectionHandle,
    _session: Arc<WinHttpSession>,
    ctx: Arc<WsContext>,
}

pub(super) async fn connect_async(
    session: Arc<WinHttpSession>,
    uri: Cow<'static, str>,
    protocols: Vec<Cow<'static, str>>,
) -> WebSocketResult<nyquest_interface::r#async::websocket::WebSocket> {
    let url_wide = prepare_websocket_url(session.base_cwurl.as_deref(), &uri)?;
    let headers_str = build_websocket_headers(&protocols, &session.options.default_headers);
    let ctx = RequestContext::new();
    let (setup_tx, setup_rx) = oneshot::channel();

    windows_threading::submit({
        let ctx = Arc::downgrade(&ctx);
        let session = session.clone();
        move || {
            let parsed_url = match ParsedUrl::parse(&url_wide) {
                Some(p) => p,
                None => {
                    let _ = setup_tx.send(Err(WebSocketError::InvalidUrl));
                    return;
                }
            };
            let method = method_to_cwstr(&nyquest_interface::Method::Get);
            let (connection, request) = match create_request(&session, &parsed_url, &method) {
                Ok(handles) => handles,
                Err(e) => {
                    let _ = setup_tx.send(Err(from_winhttp_error(e)));
                    return;
                }
            };
            if !headers_str.is_empty() {
                if let Err(e) = request.add_headers(&headers_str) {
                    let _ = setup_tx.send(Err(from_winhttp_error(e)));
                    return;
                }
            }
            if let Err(e) = request.enable_websocket_upgrade() {
                let _ = setup_tx.send(Err(from_winhttp_error(e)));
                return;
            }
            let Some(ctx) = ctx.upgrade() else {
                return;
            };
            let send_res = unsafe { request.send(std::ptr::null(), 0, Arc::into_raw(ctx) as _) }
                .map_err(from_winhttp_error)
                .map(|()| (connection, request));
            let _ = setup_tx.send(send_res);
        }
    });

    let (connection, request) = setup_rx
        .await
        .map_err(|_| WebSocketError::Io(std::io::Error::other("request setup canceled")))??;

    wait_for_state(&*ctx, RequestState::HeadersSent)
        .await
        .map_err(from_winhttp_error)?;

    request.receive_response().map_err(from_winhttp_error)?;

    wait_for_state(&*ctx, RequestState::HeadersReceived)
        .await
        .map_err(from_winhttp_error)?;

    let status = request.query_status_code().map_err(from_winhttp_error)?;
    if status != 101 {
        return Err(WebSocketError::Protocol(format!(
            "WebSocket upgrade failed: HTTP status {status}"
        )));
    }

    let protocol = query_negotiated_protocol(&request)?;

    let ws_ctx = WsContext::new();
    let raw = request
        .complete_websocket_upgrade(0)
        .map_err(from_winhttp_error)?;
    let handle = unsafe { WebSocketHandle::from_raw(raw) };
    unsafe {
        let prev = WinHttpSetStatusCallback(
            handle.as_raw(),
            Some(ws_callback),
            WINHTTP_CALLBACK_FLAG_ALL_NOTIFICATIONS,
            0,
        );
        if std::mem::transmute::<WINHTTP_STATUS_CALLBACK, usize>(prev) == usize::MAX {
            return Err(from_winhttp_error(WinHttpError::from_last_error(
                "WinHttpSetStatusCallback (websocket)",
            )));
        }
        let ws_ctx_raw = Arc::into_raw(ws_ctx.clone()) as usize;
        let opt_res = WinHttpSetOption(
            handle.as_raw(),
            WINHTTP_OPTION_CONTEXT_VALUE,
            &ws_ctx_raw as *const usize as *const c_void,
            std::mem::size_of::<usize>() as u32,
        );
        if opt_res == 0 {
            drop(Arc::from_raw(ws_ctx_raw as *const WsContext));
            return Err(from_winhttp_error(WinHttpError::from_last_error(
                "WinHttpSetOption (websocket context)",
            )));
        }
    }
    drop(request);

    let max_size = session.options.max_response_buffer_size;
    let socket = Arc::new(AsyncSocket {
        handle,
        _connection: connection,
        _session: session,
        ctx: ws_ctx,
    });

    Ok(nyquest_interface::r#async::websocket::WebSocket::new(
        WebSocketInfo { protocol },
        nyquest_interface::r#async::websocket::WebSocketSender::from_impl(AsyncSender {
            socket: socket.clone(),
            closing: false,
            close_sent: false,
        }),
        nyquest_interface::r#async::websocket::WebSocketReceiver::from_impl(AsyncReceiver {
            socket,
            max_size,
            message: Vec::new(),
            terminated: false,
        }),
    ))
}

struct AsyncSender {
    socket: Arc<AsyncSocket>,
    closing: bool,
    close_sent: bool,
}

fn closed_error() -> WebSocketError {
    WebSocketError::Protocol("WebSocket sender is closed".into())
}

impl AsyncSender {
    fn do_start_send(&mut self, data: WebSocketMessage) -> WebSocketResult<()> {
        let close = matches!(data, WebSocketMessage::Close { .. });
        let mut send = self.socket.ctx.send.lock().unwrap();
        // Caller must have ensured Idle via poll_ready / poll_close.
        send.state = SendState::InFlight;
        match data {
            WebSocketMessage::Utf8(value) => {
                send.buffer = value.into_bytes();
                let ptr = if send.buffer.is_empty() {
                    std::ptr::null()
                } else {
                    send.buffer.as_ptr().cast()
                };
                let len = send.buffer.len() as u32;
                drop(send);
                let res = unsafe {
                    WinHttpWebSocketSend(
                        self.socket.handle.as_raw(),
                        WINHTTP_WEB_SOCKET_UTF8_MESSAGE_BUFFER_TYPE,
                        ptr,
                        len,
                    )
                };
                if res != 0 {
                    self.socket.ctx.send.lock().unwrap().state = SendState::Idle;
                    return Err(winhttp_error(res, "WinHttpWebSocketSend"));
                }
            }
            WebSocketMessage::Bytes(value) => {
                send.buffer = value;
                let ptr = if send.buffer.is_empty() {
                    std::ptr::null()
                } else {
                    send.buffer.as_ptr().cast()
                };
                let len = send.buffer.len() as u32;
                drop(send);
                let res = unsafe {
                    WinHttpWebSocketSend(
                        self.socket.handle.as_raw(),
                        WINHTTP_WEB_SOCKET_BINARY_MESSAGE_BUFFER_TYPE,
                        ptr,
                        len,
                    )
                };
                if res != 0 {
                    self.socket.ctx.send.lock().unwrap().state = SendState::Idle;
                    return Err(winhttp_error(res, "WinHttpWebSocketSend"));
                }
            }
            WebSocketMessage::Close { code, reason } => {
                send.buffer = reason.into_bytes();
                let (ptr, len) = if send.buffer.is_empty() {
                    (std::ptr::null(), 0)
                } else {
                    (send.buffer.as_ptr().cast(), send.buffer.len() as u32)
                };
                drop(send);
                let res = unsafe {
                    WinHttpWebSocketShutdown(self.socket.handle.as_raw(), code, ptr, len)
                };
                if res != 0 {
                    self.socket.ctx.send.lock().unwrap().state = SendState::Idle;
                    return Err(winhttp_error(res, "WinHttpWebSocketShutdown"));
                }
            }
        }
        if close {
            self.close_sent = true;
        }
        Ok(())
    }

    fn poll_send_done(&self, cx: &mut Context<'_>) -> Poll<WebSocketResult<()>> {
        let mut send = self.socket.ctx.send.lock().unwrap();
        match std::mem::replace(&mut send.state, SendState::Idle) {
            SendState::Idle => Poll::Ready(Ok(())),
            SendState::InFlight => {
                send.state = SendState::InFlight;
                send.waker.clone_from(cx.waker());
                Poll::Pending
            }
            SendState::Done(res) => Poll::Ready(res.map_err(from_winhttp_error)),
        }
    }
}

impl Sink<WebSocketMessage> for AsyncSender {
    type Error = WebSocketError;

    fn poll_ready(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        // All fields are `Unpin`, so pinning is trivial.
        let this = self.get_mut();
        if this.closing || this.close_sent {
            return Poll::Ready(Err(closed_error()));
        }
        let mut send = this.socket.ctx.send.lock().unwrap();
        match std::mem::replace(&mut send.state, SendState::Idle) {
            SendState::Idle => Poll::Ready(Ok(())),
            SendState::InFlight => {
                send.state = SendState::InFlight;
                send.waker.clone_from(cx.waker());
                Poll::Pending
            }
            SendState::Done(Ok(())) => Poll::Ready(Ok(())),
            SendState::Done(Err(e)) => Poll::Ready(Err(from_winhttp_error(e))),
        }
    }

    fn start_send(self: Pin<&mut Self>, item: WebSocketMessage) -> Result<(), Self::Error> {
        let this = self.get_mut();
        if this.closing || this.close_sent {
            return Err(closed_error());
        }
        // poll_ready guarantees Idle (or Done(Ok) which we reset to Idle).
        this.do_start_send(item)
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        let this = self.get_mut();
        this.poll_send_done(cx)
    }

    fn poll_close(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        let this = self.get_mut();
        this.closing = true;
        // A close frame sent via `start_send` already initiates the handshake;
        // only flush it instead of emitting a second one.
        if !this.close_sent {
            // Wait for any in-flight user data before enqueueing close.
            {
                let mut send = this.socket.ctx.send.lock().unwrap();
                match std::mem::replace(&mut send.state, SendState::Idle) {
                    SendState::InFlight => {
                        send.state = SendState::InFlight;
                        send.waker.clone_from(cx.waker());
                        return Poll::Pending;
                    }
                    SendState::Idle => {
                        // Fall through to enqueue close below.
                    }
                    SendState::Done(Ok(())) => {
                        // Previous send finished; fall through.
                    }
                    SendState::Done(Err(e)) => {
                        return Poll::Ready(Err(from_winhttp_error(e)));
                    }
                }
            }
            // `do_start_send` records the emitted close frame in `close_sent`.
            if let Err(e) = this.do_start_send(WebSocketMessage::Close {
                code: 1000,
                reason: String::new(),
            }) {
                return Poll::Ready(Err(e));
            }
        }
        match this.poll_send_done(cx) {
            Poll::Ready(Ok(())) => Poll::Ready(Ok(())),
            Poll::Ready(Err(e)) => Poll::Ready(Err(e)),
            Poll::Pending => Poll::Pending,
        }
    }
}

struct AsyncReceiver {
    socket: Arc<AsyncSocket>,
    max_size: Option<u64>,
    message: Vec<u8>,
    /// Set once a close frame or a fatal transport error has been yielded;
    /// subsequent `poll_next` calls return `None`.
    terminated: bool,
}

impl Stream for AsyncReceiver {
    type Item = WebSocketResult<WebSocketMessage>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = self.get_mut();
        if this.terminated {
            return Poll::Ready(None);
        }
        loop {
            let start_ptr = {
                let mut recv = this.socket.ctx.recv.lock().unwrap();
                if matches!(recv.state, RecvState::Idle) {
                    recv.state = RecvState::InFlight;
                    let buffer: &mut [MaybeUninit<u8>; _] =
                        (**recv.buffer.get_or_insert_with(Box::new_uninit)).as_mut();
                    Some(buffer.as_mut_ptr().cast::<c_void>())
                } else {
                    None
                }
            };
            if let Some(ptr) = start_ptr {
                let res = unsafe {
                    WinHttpWebSocketReceive(
                        this.socket.handle.as_raw(),
                        ptr,
                        WEBSOCKET_RECV_BUFFER_SIZE as u32,
                        std::ptr::null_mut(),
                        std::ptr::null_mut(),
                    )
                };
                if res != 0 {
                    this.socket.ctx.recv.lock().unwrap().state = RecvState::Idle;
                    this.terminated = true;
                    return Poll::Ready(Some(Err(winhttp_error(res, "WinHttpWebSocketReceive"))));
                }
            }

            enum Outcome {
                ReadyMessage(WebSocketMessage),
                ReadyClose(WebSocketMessage),
                /// A recoverable per-message failure; the stream continues.
                ReadyError(WebSocketError),
                /// A fatal transport failure; the stream terminates after yielding it.
                ReadyFatalError(WebSocketError),
                Continue,
                Pending,
            }

            let outcome = {
                let mut recv = this.socket.ctx.recv.lock().unwrap();
                match std::mem::replace(&mut recv.state, RecvState::Idle) {
                    RecvState::Done(Ok((read_len, buffer_type))) => {
                        if this.max_size.is_some_and(|limit| {
                            this.message.len().saturating_add(read_len) > limit as usize
                        }) {
                            this.message.clear();
                            Outcome::ReadyError(WebSocketError::MessageTooLarge)
                        } else {
                            let buffer: &[MaybeUninit<u8>; WEBSOCKET_RECV_BUFFER_SIZE] =
                                (**recv.buffer.as_ref().unwrap()).as_ref();
                            // SAFETY: read_len bytes were initialized by WinHTTP.
                            let read_buf = unsafe { buffer[..read_len].assume_init_ref() };
                            this.message.extend_from_slice(read_buf);
                            match buffer_type {
                                WINHTTP_WEB_SOCKET_UTF8_MESSAGE_BUFFER_TYPE => {
                                    let message = std::mem::take(&mut this.message);
                                    match String::from_utf8(message) {
                                        Ok(s) => Outcome::ReadyMessage(WebSocketMessage::Utf8(s)),
                                        Err(_) => Outcome::ReadyError(WebSocketError::Protocol(
                                            "invalid UTF-8 message".into(),
                                        )),
                                    }
                                }
                                WINHTTP_WEB_SOCKET_BINARY_MESSAGE_BUFFER_TYPE => {
                                    Outcome::ReadyMessage(WebSocketMessage::Bytes(std::mem::take(
                                        &mut this.message,
                                    )))
                                }
                                WINHTTP_WEB_SOCKET_CLOSE_BUFFER_TYPE => {
                                    this.message.clear();
                                    drop(recv);
                                    match this.socket.handle.query_close_status() {
                                        Ok(msg) => Outcome::ReadyClose(msg),
                                        Err(e) => Outcome::ReadyFatalError(e),
                                    }
                                }
                                WINHTTP_WEB_SOCKET_UTF8_FRAGMENT_BUFFER_TYPE
                                | WINHTTP_WEB_SOCKET_BINARY_FRAGMENT_BUFFER_TYPE => {
                                    Outcome::Continue
                                }
                                _ => {
                                    this.message.clear();
                                    Outcome::ReadyError(WebSocketError::Protocol(
                                        "unknown WebSocket buffer type".into(),
                                    ))
                                }
                            }
                        }
                    }
                    RecvState::Done(Err(e)) => Outcome::ReadyFatalError(from_winhttp_error(e)),
                    RecvState::InFlight => {
                        recv.state = RecvState::InFlight;
                        recv.waker.clone_from(cx.waker());
                        Outcome::Pending
                    }
                    RecvState::Idle => Outcome::Continue,
                }
            };

            match outcome {
                Outcome::ReadyMessage(msg) => return Poll::Ready(Some(Ok(msg))),
                Outcome::ReadyClose(msg) => {
                    this.terminated = true;
                    return Poll::Ready(Some(Ok(msg)));
                }
                Outcome::ReadyError(e) => return Poll::Ready(Some(Err(e))),
                Outcome::ReadyFatalError(e) => {
                    this.terminated = true;
                    return Poll::Ready(Some(Err(e)));
                }
                Outcome::Continue => continue,
                Outcome::Pending => return Poll::Pending,
            }
        }
    }
}
