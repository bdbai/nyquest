use std::borrow::Cow;
use std::ffi::c_void;
use std::future::{poll_fn, Future};
use std::sync::{Arc, Mutex};
use std::task::{Poll, Waker};

use futures_channel::oneshot;
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
    winhttp_error, WebSocketHandle,
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
    buffer: Vec<u8>,
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
                buffer: vec![0u8; 16 * 1024],
            }),
        })
    }

    fn complete_send(&self, res: Result<(), WinHttpError>) {
        let mut send = self.send.lock().unwrap();
        send.buffer.clear();
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
        WINHTTP_CALLBACK_STATUS_WRITE_COMPLETE
        | WINHTTP_CALLBACK_STATUS_SHUTDOWN_COMPLETE
        | WINHTTP_CALLBACK_STATUS_CLOSE_COMPLETE => {
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
    _session: Arc<WinHttpSession>,
    _connection: ConnectionHandle,
    handle: WebSocketHandle,
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
    let ws_ctx_raw = Arc::into_raw(ws_ctx.clone());
    let raw = match request.complete_websocket_upgrade(ws_ctx_raw as usize) {
        Ok(raw) => raw,
        Err(e) => {
            unsafe {
                drop(Arc::from_raw(ws_ctx_raw));
            }
            return Err(from_winhttp_error(e));
        }
    };
    let handle = WebSocketHandle::from_raw(raw);
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
    }
    drop(request);

    let max_size = session.options.max_response_buffer_size;
    let socket = Arc::new(AsyncSocket {
        _session: session,
        _connection: connection,
        handle,
        ctx: ws_ctx,
    });

    Ok(nyquest_interface::r#async::websocket::WebSocket::new(
        WebSocketInfo { protocol },
        nyquest_interface::r#async::websocket::WebSocketSender::from_impl(AsyncSender {
            socket: socket.clone(),
        }),
        nyquest_interface::r#async::websocket::WebSocketReceiver::from_impl(AsyncReceiver {
            socket,
            max_size,
        }),
    ))
}

struct AsyncSender {
    socket: Arc<AsyncSocket>,
}

impl nyquest_interface::r#async::websocket::AsyncWebSocketSender for AsyncSender {
    fn send(&mut self, data: WebSocketMessage) -> impl Future<Output = WebSocketResult<()>> + Send {
        let socket = self.socket.clone();
        async move {
            {
                let mut send = socket.ctx.send.lock().unwrap();
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
                                socket.handle.as_raw(),
                                WINHTTP_WEB_SOCKET_UTF8_MESSAGE_BUFFER_TYPE,
                                ptr,
                                len,
                            )
                        };
                        if res != 0 {
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
                                socket.handle.as_raw(),
                                WINHTTP_WEB_SOCKET_BINARY_MESSAGE_BUFFER_TYPE,
                                ptr,
                                len,
                            )
                        };
                        if res != 0 {
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
                            WinHttpWebSocketShutdown(socket.handle.as_raw(), code, ptr, len)
                        };
                        if res != 0 {
                            return Err(winhttp_error(res, "WinHttpWebSocketShutdown"));
                        }
                    }
                }
            }

            poll_fn(|cx| {
                let mut send = socket.ctx.send.lock().unwrap();
                match std::mem::replace(&mut send.state, SendState::Idle) {
                    SendState::Done(res) => Poll::Ready(res.map_err(from_winhttp_error)),
                    state => {
                        send.state = state;
                        send.waker.clone_from(cx.waker());
                        Poll::Pending
                    }
                }
            })
            .await
        }
    }

    fn close(&mut self) -> impl Future<Output = WebSocketResult<()>> + Send {
        self.send(WebSocketMessage::Close {
            code: 1000,
            reason: String::new(),
        })
    }
}

struct AsyncReceiver {
    socket: Arc<AsyncSocket>,
    max_size: Option<u64>,
}

impl nyquest_interface::r#async::websocket::AsyncWebSocketReceiver for AsyncReceiver {
    fn receive(&mut self) -> impl Future<Output = WebSocketResult<WebSocketMessage>> + Send {
        let socket = self.socket.clone();
        let max_size = self.max_size;
        async move {
            let mut message = Vec::new();
            loop {
                {
                    let mut recv = socket.ctx.recv.lock().unwrap();
                    if recv.buffer.len() < 16 * 1024 {
                        recv.buffer.resize(16 * 1024, 0);
                    }
                    recv.state = RecvState::InFlight;
                    let ptr = recv.buffer.as_mut_ptr();
                    let len = recv.buffer.len() as u32;
                    drop(recv);

                    let res = unsafe {
                        WinHttpWebSocketReceive(
                            socket.handle.as_raw(),
                            ptr.cast(),
                            len,
                            std::ptr::null_mut(),
                            std::ptr::null_mut(),
                        )
                    };
                    if res != 0 {
                        return Err(winhttp_error(res, "WinHttpWebSocketReceive"));
                    }
                }

                let (read_len, buffer_type) = poll_fn(|cx| {
                    let mut recv = socket.ctx.recv.lock().unwrap();
                    match std::mem::replace(&mut recv.state, RecvState::Idle) {
                        RecvState::Done(res) => Poll::Ready(res.map_err(from_winhttp_error)),
                        state => {
                            recv.state = state;
                            recv.waker.clone_from(cx.waker());
                            Poll::Pending
                        }
                    }
                })
                .await?;

                if max_size
                    .is_some_and(|limit| message.len().saturating_add(read_len) > limit as usize)
                {
                    return Err(WebSocketError::MessageTooLarge);
                }

                {
                    let recv = socket.ctx.recv.lock().unwrap();
                    message.extend_from_slice(&recv.buffer[..read_len]);
                }

                match buffer_type {
                    WINHTTP_WEB_SOCKET_UTF8_MESSAGE_BUFFER_TYPE => {
                        return String::from_utf8(message)
                            .map(WebSocketMessage::Utf8)
                            .map_err(|_| WebSocketError::Protocol("invalid UTF-8 message".into()));
                    }
                    WINHTTP_WEB_SOCKET_BINARY_MESSAGE_BUFFER_TYPE => {
                        return Ok(WebSocketMessage::Bytes(message));
                    }
                    WINHTTP_WEB_SOCKET_CLOSE_BUFFER_TYPE => {
                        return socket.handle.query_close_status();
                    }
                    WINHTTP_WEB_SOCKET_UTF8_FRAGMENT_BUFFER_TYPE
                    | WINHTTP_WEB_SOCKET_BINARY_FRAGMENT_BUFFER_TYPE => {}
                    _ => {
                        return Err(WebSocketError::Protocol(
                            "unknown WebSocket buffer type".into(),
                        ))
                    }
                }
            }
        }
    }
}
