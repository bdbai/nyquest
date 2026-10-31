use std::borrow::Cow;
use std::collections::VecDeque;
use std::ffi::c_int;
use std::future::{poll_fn, Future};
use std::sync::{Arc, Mutex};
use std::task::Poll;

use futures_channel::oneshot;
use futures_util::task::AtomicWaker;
use nyquest_interface::websocket::{WebSocketError, WebSocketMessage, WebSocketResult};

use super::r#loop::{LoopManager, LoopTask, RequestHandle};
use crate::curl_ng::easy::{AsRawEasyMut, Share};
use crate::request::{create_easy, AsCallbackMut};
use crate::state::RequestState;
use crate::websocket::{
    configure_websocket_easy, curl_error, curl_ws_meta, curl_ws_start_frame, decode_close_payload,
    encode_message, nyquest_to_ws_error, CURLWS_BINARY, CURLWS_CLOSE, CURLWS_CONT, CURLWS_PING,
    CURLWS_PONG, CURLWS_TEXT,
};

pub(crate) struct OutgoingMessage {
    flags: c_int,
    encoded: Vec<u8>,
    offset: usize,
    frame_started: bool,
    response: Option<oneshot::Sender<WebSocketResult<()>>>,
}

#[derive(Default)]
struct WsState {
    header_state: RequestState,
    outgoing_queue: VecDeque<OutgoingMessage>,
    current_outgoing: Option<OutgoingMessage>,
    incoming_messages: VecDeque<WebSocketResult<WebSocketMessage>>,
    current_incoming_payload: Vec<u8>,
    current_incoming_is_text: bool,
    discarding_oversized: bool,
    close_payload: Vec<u8>,
    done: bool,
    terminal_error: Option<String>,
}

pub(crate) struct WsShared {
    max_message_size: Option<usize>,
    state: Mutex<WsState>,
    waker: AtomicWaker,
    handshake_tx: Mutex<Option<oneshot::Sender<WebSocketResult<()>>>>,
}

impl WsShared {
    pub(crate) fn on_header(&self, data: &[u8]) -> bool {
        let mut state = self.state.lock().unwrap();
        if state.header_state.push_header_data(data) {
            let status = state.header_state.temp_status_code;
            drop(state);
            if let Some(tx) = self.handshake_tx.lock().unwrap().take() {
                if status == 101 {
                    let _ = tx.send(Ok(()));
                } else {
                    let _ = tx.send(Err(WebSocketError::Protocol(format!(
                        "WebSocket upgrade failed: HTTP status {status}"
                    ))));
                    return false;
                }
            }
        }
        true
    }

    pub(crate) fn on_read(
        &self,
        buf: &mut [u8],
        raw: *mut curl_sys::CURL,
    ) -> Result<usize, curl::easy::ReadError> {
        let mut state = self.state.lock().unwrap();
        if state.current_outgoing.is_none() {
            state.current_outgoing = state.outgoing_queue.pop_front();
        }
        let Some(outgoing) = &mut state.current_outgoing else {
            return Err(curl::easy::ReadError::Pause);
        };

        if !outgoing.frame_started {
            let res = unsafe {
                curl_ws_start_frame(
                    raw,
                    outgoing.flags,
                    outgoing.encoded.len() as curl_sys::curl_off_t,
                )
            };
            if res == curl_sys::CURLE_AGAIN {
                return Ok(0);
            }
            if res != curl_sys::CURLE_OK {
                if let Some(resp) = outgoing.response.take() {
                    let _ = resp.send(Err(curl_error(res)));
                }
                state.current_outgoing = None;
                return Err(curl::easy::ReadError::Abort);
            }
            outgoing.frame_started = true;
        }

        if outgoing.encoded.is_empty() {
            if let Some(resp) = outgoing.response.take() {
                let _ = resp.send(Ok(()));
            }
            state.current_outgoing = None;
            return Ok(0);
        }

        let remaining = &outgoing.encoded[outgoing.offset..];
        let to_copy = remaining.len().min(buf.len());
        buf[..to_copy].copy_from_slice(&remaining[..to_copy]);
        outgoing.offset += to_copy;

        if outgoing.offset == outgoing.encoded.len() {
            if let Some(resp) = outgoing.response.take() {
                let _ = resp.send(Ok(()));
            }
            state.current_outgoing = None;
        }

        Ok(to_copy)
    }

    pub(crate) fn on_write(
        &self,
        data: &[u8],
        raw: *mut curl_sys::CURL,
    ) -> Result<usize, curl::easy::WriteError> {
        let meta = unsafe { curl_ws_meta(raw) };
        if meta.is_null() {
            return Ok(data.len());
        }
        let (flags, bytesleft) = unsafe { ((*meta).flags, (*meta).bytesleft) };

        if flags & (CURLWS_PING | CURLWS_PONG) != 0 {
            return Ok(data.len());
        }

        let mut state = self.state.lock().unwrap();

        if flags & CURLWS_CLOSE != 0 {
            state.close_payload.extend_from_slice(data);
            if bytesleft == 0 {
                state.done = true;
                let msg = decode_close_payload(&state.close_payload);
                state.close_payload.clear();
                state.incoming_messages.push_back(Ok(msg));
                drop(state);
                self.waker.wake();
            }
            return Ok(data.len());
        }

        let is_final_chunk = bytesleft == 0 && (flags & CURLWS_CONT == 0);

        if state.discarding_oversized {
            if is_final_chunk {
                state.discarding_oversized = false;
                state.current_incoming_payload.clear();
            }
            return Ok(data.len());
        }

        if state.current_incoming_payload.is_empty() {
            if flags & CURLWS_TEXT != 0 {
                state.current_incoming_is_text = true;
            } else if flags & CURLWS_BINARY != 0 {
                state.current_incoming_is_text = false;
            }
        }

        if let Some(max_size) = self.max_message_size {
            if state
                .current_incoming_payload
                .len()
                .saturating_add(data.len())
                > max_size
            {
                state.current_incoming_payload.clear();
                if !is_final_chunk {
                    state.discarding_oversized = true;
                }
                state
                    .incoming_messages
                    .push_back(Err(WebSocketError::MessageTooLarge));
                drop(state);
                self.waker.wake();
                return Ok(data.len());
            }
        }

        state.current_incoming_payload.extend_from_slice(data);

        if is_final_chunk {
            let payload = std::mem::take(&mut state.current_incoming_payload);
            let msg = if state.current_incoming_is_text {
                String::from_utf8(payload)
                    .map(WebSocketMessage::Utf8)
                    .map_err(|_| WebSocketError::Protocol("invalid UTF-8".into()))
            } else {
                Ok(WebSocketMessage::Bytes(payload))
            };
            state.incoming_messages.push_back(msg);
            drop(state);
            self.waker.wake();
        }

        Ok(data.len())
    }

    pub(crate) fn on_done(&self, res: Result<(), &nyquest_interface::Error>) {
        if let Some(tx) = self.handshake_tx.lock().unwrap().take() {
            let err = match res {
                Err(e) => WebSocketError::Io(std::io::Error::other(e.to_string())),
                Ok(()) => {
                    WebSocketError::Protocol("connection closed before handshake completed".into())
                }
            };
            let _ = tx.send(Err(err));
        }

        let mut state = self.state.lock().unwrap();
        state.done = true;
        if let Err(e) = res {
            state.terminal_error = Some(e.to_string());
        }
        let err_msg = state
            .terminal_error
            .clone()
            .unwrap_or_else(|| "WebSocket connection closed".into());
        if let Some(mut current) = state.current_outgoing.take() {
            if let Some(resp) = current.response.take() {
                let _ = resp.send(Err(WebSocketError::Io(std::io::Error::new(
                    std::io::ErrorKind::ConnectionAborted,
                    err_msg.clone(),
                ))));
            }
        }
        for mut outgoing in state.outgoing_queue.drain(..) {
            if let Some(resp) = outgoing.response.take() {
                let _ = resp.send(Err(WebSocketError::Io(std::io::Error::new(
                    std::io::ErrorKind::ConnectionAborted,
                    err_msg.clone(),
                ))));
            }
        }
        drop(state);
        self.waker.wake();
    }
}

pub(super) async fn connect_async(
    loop_manager: &LoopManager,
    share: &Share,
    options: &nyquest_interface::client::ClientOptions,
    uri: Cow<'static, str>,
    protocol: Option<Cow<'static, str>>,
) -> WebSocketResult<nyquest_interface::r#async::websocket::WebSocket> {
    let (handshake_tx, handshake_rx) = oneshot::channel();
    let max_message_size = options
        .max_response_buffer_size
        .and_then(|size| usize::try_from(size).ok());

    let ws_shared = Arc::new(WsShared {
        max_message_size,
        state: Mutex::new(WsState {
            current_incoming_is_text: true,
            ..Default::default()
        }),
        waker: AtomicWaker::new(),
        handshake_tx: Mutex::new(Some(handshake_tx)),
    });

    let mut handler = super::handler::AsyncHandler::default();
    handler.ws = Some(ws_shared.clone());

    let mut easy = create_easy(handler, share).map_err(nyquest_to_ws_error)?;
    let raw = easy.as_mut().as_raw_easy_mut().raw();
    easy.as_callback_mut().pause = Some(super::pause::EasyPause::new(raw));

    configure_websocket_easy(easy.as_mut(), options, uri, protocol)?;
    easy.as_mut()
        .with_error_message(|e| e.as_raw_easy_mut().set_upload(true))
        .map_err(|e| nyquest_to_ws_error(e.into()))?;

    let req_handle = loop_manager
        .start_request(easy)
        .await
        .map_err(nyquest_to_ws_error)?;

    // Wait for the HTTP 101 WebSocket handshake to complete.
    // Holding `req_handle` ensures the easy handle is removed if the handshake fails or is cancelled.
    handshake_rx
        .await
        .map_err(|_| WebSocketError::Io(std::io::Error::other("WebSocket handshake canceled")))??;

    let req_handle = Arc::new(req_handle);
    let sender = AsyncSender {
        ws: ws_shared.clone(),
        handle: req_handle.clone(),
    };
    let receiver = AsyncReceiver {
        ws: ws_shared,
        _handle: req_handle,
    };

    Ok(nyquest_interface::r#async::websocket::WebSocket::new(
        nyquest_interface::r#async::websocket::WebSocketSender::from_impl(sender),
        nyquest_interface::r#async::websocket::WebSocketReceiver::from_impl(receiver),
    ))
}

struct AsyncSender {
    ws: Arc<WsShared>,
    handle: Arc<RequestHandle>,
}

impl nyquest_interface::r#async::websocket::AsyncWebSocketSender for AsyncSender {
    fn send(&mut self, data: WebSocketMessage) -> impl Future<Output = WebSocketResult<()>> + Send {
        let ws = self.ws.clone();
        let handle = self.handle.clone();
        async move {
            let (tx, rx) = oneshot::channel();
            let (flags, encoded) = encode_message(data);
            {
                let mut state = ws.state.lock().unwrap();
                if state.done {
                    let msg = state
                        .terminal_error
                        .clone()
                        .unwrap_or_else(|| "WebSocket connection closed".into());
                    return Err(WebSocketError::Io(std::io::Error::new(
                        std::io::ErrorKind::ConnectionAborted,
                        msg,
                    )));
                }
                state.outgoing_queue.push_back(OutgoingMessage {
                    flags,
                    encoded,
                    offset: 0,
                    frame_started: false,
                    response: Some(tx),
                });
            }

            handle
                .manager()
                .dispatch_task(LoopTask::UnpauseSendHandle(handle.id()))
                .map_err(|_| {
                    WebSocketError::Io(std::io::Error::other("Loop manager task dispatch failed"))
                })?;

            rx.await
                .map_err(|_| WebSocketError::Io(std::io::Error::other("WebSocket send canceled")))?
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
    ws: Arc<WsShared>,
    _handle: Arc<RequestHandle>,
}

impl nyquest_interface::r#async::websocket::AsyncWebSocketReceiver for AsyncReceiver {
    fn receive(&mut self) -> impl Future<Output = WebSocketResult<WebSocketMessage>> + Send {
        let ws = self.ws.clone();
        poll_fn(move |cx| {
            let mut state = ws.state.lock().unwrap();
            if let Some(msg) = state.incoming_messages.pop_front() {
                return Poll::Ready(msg);
            }
            if state.done {
                if let Some(err) = state.terminal_error.take() {
                    return Poll::Ready(Err(WebSocketError::Io(std::io::Error::other(err))));
                }
                return Poll::Ready(Err(WebSocketError::Protocol(
                    "WebSocket stream closed".into(),
                )));
            }
            ws.waker.register(cx.waker());
            Poll::Pending
        })
    }
}
