use std::borrow::Cow;
use std::collections::VecDeque;
use std::ffi::c_int;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll, Waker};

use futures_core::Stream;
use futures_sink::Sink;
use nyquest_interface::websocket::{
    WebSocketError, WebSocketInfo, WebSocketMessage, WebSocketResult,
};

use super::r#loop::{LoopManager, LoopTask, RequestHandle};
use crate::curl_ng::easy::{AsRawEasyMut, Share};
use crate::r#async::shared::{RequestResult, SharedRequestStates};
use crate::request::{create_easy, AsCallbackMut};
use crate::websocket::{
    configure_websocket_easy, curl_ws_meta, curl_ws_start_frame, decode_close_payload,
    encode_message, extract_negotiated_protocol, nyquest_to_ws_error, CURLWS_BINARY, CURLWS_CLOSE,
    CURLWS_CONT, CURLWS_PING, CURLWS_PONG, CURLWS_TEXT,
};

pub(crate) struct OutgoingMessage {
    flags: c_int,
    encoded: Vec<u8>,
    offset: usize,
    frame_started: bool,
}

pub(super) struct WsState {
    max_message_size: Option<usize>,
    outgoing_queue: Option<OutgoingMessage>,
    outgoing_waker: Waker,
    incoming_messages: VecDeque<WebSocketResult<WebSocketMessage>>,
    current_incoming_payload: Vec<u8>,
    current_incoming_is_text: bool,
    discarding_oversized: bool,
    close_payload: Vec<u8>,
    done: bool,
}

impl WsState {
    pub(super) fn wake_outgoing(&self) {
        self.outgoing_waker.wake_by_ref();
    }

    pub(crate) fn on_read(
        &mut self,
        buf: &mut [u8],
        raw: *mut curl_sys::CURL,
    ) -> Result<usize, curl::easy::ReadError> {
        let Some(outgoing) = &mut self.outgoing_queue else {
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
                self.outgoing_queue = None;
                self.outgoing_waker.wake_by_ref();
                return Err(curl::easy::ReadError::Abort);
            }
            outgoing.frame_started = true;
        }

        if outgoing.encoded.is_empty() {
            self.outgoing_queue = None;
            self.outgoing_waker.wake_by_ref();
            return Ok(0);
        }

        let remaining = &outgoing.encoded[outgoing.offset..];
        let to_copy = remaining.len().min(buf.len());
        buf[..to_copy].copy_from_slice(&remaining[..to_copy]);
        outgoing.offset += to_copy;

        if outgoing.offset == outgoing.encoded.len() {
            self.outgoing_queue = None;
            self.outgoing_waker.wake_by_ref();
        }

        Ok(to_copy)
    }

    pub(crate) fn on_write(
        &mut self,
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

        if flags & CURLWS_CLOSE != 0 {
            self.close_payload.extend_from_slice(data);
            if bytesleft == 0 {
                self.done = true;
                let msg = decode_close_payload(&self.close_payload);
                self.close_payload.clear();
                self.incoming_messages.push_back(Ok(msg));
            }
            return Ok(data.len());
        }

        let is_final_chunk = bytesleft == 0 && (flags & CURLWS_CONT == 0);

        if self.discarding_oversized {
            if is_final_chunk {
                self.discarding_oversized = false;
                self.current_incoming_payload.clear();
            }
            return Ok(data.len());
        }

        if self.current_incoming_payload.is_empty() {
            if flags & CURLWS_TEXT != 0 {
                self.current_incoming_is_text = true;
            } else if flags & CURLWS_BINARY != 0 {
                self.current_incoming_is_text = false;
            }
        }

        if let Some(max_size) = self.max_message_size {
            if self
                .current_incoming_payload
                .len()
                .saturating_add(data.len())
                > max_size
            {
                self.current_incoming_payload.clear();
                if !is_final_chunk {
                    self.discarding_oversized = true;
                }
                self.incoming_messages
                    .push_back(Err(WebSocketError::MessageTooLarge));
                return Ok(data.len());
            }
        }

        self.current_incoming_payload.extend_from_slice(data);

        if is_final_chunk {
            let payload = std::mem::take(&mut self.current_incoming_payload);
            let msg = if self.current_incoming_is_text {
                String::from_utf8(payload)
                    .map(WebSocketMessage::Utf8)
                    .map_err(|_| WebSocketError::Protocol("invalid UTF-8".into()))
            } else {
                Ok(WebSocketMessage::Bytes(payload))
            };
            self.incoming_messages.push_back(msg);
        }

        Ok(data.len())
    }
}

pub(super) async fn connect_async(
    loop_manager: &LoopManager,
    share: &Share,
    options: &nyquest_interface::client::ClientOptions,
    uri: Cow<'static, str>,
    protocols: Vec<Cow<'static, str>>,
) -> WebSocketResult<nyquest_interface::r#async::websocket::WebSocket> {
    let max_message_size = options
        .max_response_buffer_size
        .and_then(|size| usize::try_from(size).ok());

    let shared = {
        let mut shared = SharedRequestStates::default();
        shared.state.get_mut().unwrap().ws = Some(WsState {
            outgoing_waker: futures_util::task::noop_waker(),
            max_message_size,
            current_incoming_payload: Vec::new(),
            current_incoming_is_text: false,
            incoming_messages: VecDeque::new(),
            discarding_oversized: false,
            close_payload: Vec::new(),
            done: false,
            outgoing_queue: None,
        });
        Arc::new(shared)
    };
    let handler = super::handler::AsyncHandler {
        ctx: shared.clone(),
        ..Default::default()
    };

    let mut easy = create_easy(handler, share).map_err(nyquest_to_ws_error)?;
    let raw = easy.as_mut().as_raw_easy_mut().raw();
    easy.as_callback_mut().pause = Some(super::pause::EasyPause::new(raw));

    configure_websocket_easy(easy.as_mut(), options, uri, protocols)?;
    easy.as_mut()
        .with_error_message(|e| e.as_raw_easy_mut().set_upload(true))
        .map_err(|e| nyquest_to_ws_error(e.into()))?;

    let req_handle = loop_manager
        .start_request(easy)
        .await
        .map_err(nyquest_to_ws_error)?;

    let res = req_handle
        .wait_for_response()
        .await
        .map_err(nyquest_to_ws_error)?;

    if res.status != 101 {
        return Err(WebSocketError::Protocol(format!(
            "WebSocket upgrade failed: HTTP status {}",
            res.status
        )));
    }

    let protocol = extract_negotiated_protocol(&res.headers);

    let req_handle = Arc::new(res.handle);
    let sender = AsyncSender {
        shared,
        handle: req_handle.clone(),
        closing: false,
        close_sent: false,
    };
    let receiver = AsyncReceiver { handle: req_handle };

    Ok(nyquest_interface::r#async::websocket::WebSocket::new(
        WebSocketInfo { protocol },
        nyquest_interface::r#async::websocket::WebSocketSender::from_impl(sender),
        nyquest_interface::r#async::websocket::WebSocketReceiver::from_impl(receiver),
    ))
}

struct AsyncSender {
    handle: Arc<RequestHandle>,
    shared: Arc<SharedRequestStates>,
    closing: bool,
    close_sent: bool,
}

impl AsyncSender {
    fn is_in_progress(&self) -> Option<usize> {
        let state = self.shared.state.lock().unwrap();
        match state.result {
            RequestResult::InProgress { id } => Some(id),
            _ => None,
        }
    }

    fn ensure_unpaused(&self, id: usize) {
        let _ = self
            .handle
            .manager()
            .dispatch_task(LoopTask::UnpauseSendHandle(id));
    }
}

fn closed_error() -> WebSocketError {
    WebSocketError::Protocol("WebSocket sender is closed".into())
}

impl Sink<WebSocketMessage> for AsyncSender {
    type Error = WebSocketError;

    fn poll_ready(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        // All fields are `Unpin`, so pinning is trivial.
        let this = self.get_mut();
        if this.closing || this.close_sent {
            return Poll::Ready(Err(closed_error()));
        }
        let Some(_id) = this.is_in_progress() else {
            // Connection is gone; behave as ready and drop subsequent sends,
            // errors surface on the receive half.
            return Poll::Ready(Ok(()));
        };
        let mut state = this.shared.state.lock().unwrap();
        let ws = state
            .ws
            .as_mut()
            .expect("WebSocket state should be present");
        if ws.outgoing_queue.is_some() {
            ws.outgoing_waker.clone_from(cx.waker());
            Poll::Pending
        } else {
            Poll::Ready(Ok(()))
        }
    }

    fn start_send(self: Pin<&mut Self>, item: WebSocketMessage) -> Result<(), Self::Error> {
        let this = self.get_mut();
        if this.closing || this.close_sent {
            return Err(closed_error());
        }
        let Some(_id) = this.is_in_progress() else {
            // Drop the message; the receive half will report the failure.
            return Ok(());
        };
        let close = matches!(item, WebSocketMessage::Close { .. });
        let (flags, encoded) = encode_message(item);
        let mut state = this.shared.state.lock().unwrap();
        let ws = state
            .ws
            .as_mut()
            .expect("WebSocket state should be present");
        if ws.outgoing_queue.is_some() {
            return Err(WebSocketError::Protocol(
                "WebSocket send buffer is full".into(),
            ));
        }
        ws.outgoing_queue = Some(OutgoingMessage {
            flags,
            encoded,
            offset: 0,
            frame_started: false,
        });
        if close {
            this.close_sent = true;
        }
        Ok(())
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        let this = self.get_mut();
        let Some(id) = this.is_in_progress() else {
            return Poll::Ready(Ok(()));
        };
        let mut state = this.shared.state.lock().unwrap();
        let ws = state
            .ws
            .as_mut()
            .expect("WebSocket state should be present");
        if ws.outgoing_queue.is_none() {
            return Poll::Ready(Ok(()));
        }
        ws.outgoing_waker.clone_from(cx.waker());
        drop(state);
        this.ensure_unpaused(id);
        Poll::Pending
    }

    fn poll_close(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        let this = self.get_mut();
        this.closing = true;
        // A close frame already queued via `start_send` initiates the handshake;
        // only flush it instead of emitting a second one.
        if this.close_sent {
            let Some(id) = this.is_in_progress() else {
                return Poll::Ready(Ok(()));
            };
            let mut state = this.shared.state.lock().unwrap();
            let ws = state
                .ws
                .as_mut()
                .expect("WebSocket state should be present");
            if ws.outgoing_queue.is_none() {
                return Poll::Ready(Ok(()));
            }
            ws.outgoing_waker.clone_from(cx.waker());
            drop(state);
            this.ensure_unpaused(id);
            return Poll::Pending;
        }
        let Some(id) = this.is_in_progress() else {
            return Poll::Ready(Ok(()));
        };
        // If there is still user data in flight, wait for it before enqueueing close.
        {
            let mut state = this.shared.state.lock().unwrap();
            let ws = state
                .ws
                .as_mut()
                .expect("WebSocket state should be present");
            if ws.outgoing_queue.is_some() {
                ws.outgoing_waker.clone_from(cx.waker());
                drop(state);
                this.ensure_unpaused(id);
                return Poll::Pending;
            }
            let (flags, encoded) = encode_message(WebSocketMessage::Close {
                code: 1000,
                reason: String::new(),
            });
            ws.outgoing_queue = Some(OutgoingMessage {
                flags,
                encoded,
                offset: 0,
                frame_started: false,
            });
            this.close_sent = true;
            ws.outgoing_waker.clone_from(cx.waker());
            drop(state);
            this.ensure_unpaused(id);
            Poll::Pending
        }
    }
}

struct AsyncReceiver {
    handle: Arc<RequestHandle>,
}

impl Stream for AsyncReceiver {
    type Item = WebSocketResult<WebSocketMessage>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        // `AsyncReceiver` only holds an `Arc`, so pinning is trivial.
        let this = self.get_mut();
        let res = this.handle.poll_recv(cx, |state| {
            let ws = state
                .ws
                .as_mut()
                .expect("WebSocket state should be present");
            if let Some(msg) = ws.incoming_messages.pop_front() {
                return Some(msg);
            }
            if ws.done {
                return Some(Err(WebSocketError::Protocol(
                    "WebSocket stream closed".into(),
                )));
            }
            None
        });
        match res {
            Poll::Ready(Ok(Some(Ok(msg)))) => Poll::Ready(Some(Ok(msg))),
            Poll::Ready(Ok(Some(Err(e)))) => {
                if matches!(&e, WebSocketError::Protocol(s) if s == "WebSocket stream closed") {
                    Poll::Ready(None)
                } else {
                    Poll::Ready(Some(Err(e)))
                }
            }
            Poll::Ready(Ok(None)) => Poll::Ready(None),
            Poll::Ready(Err(e)) => Poll::Ready(Some(Err(nyquest_to_ws_error(e)))),
            Poll::Pending => Poll::Pending,
        }
    }
}
