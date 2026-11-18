use std::borrow::Cow;
use std::pin::Pin;
use std::task::{Context, Poll};

use futures::{Sink, Stream, StreamExt};
use gloo_net::websocket::futures::WebSocket as GlooWebSocket;
use gloo_net::websocket::{Message as GlooMessage, WebSocketError as GlooError};
use nyquest_interface::websocket::{
    WebSocketError, WebSocketInfo, WebSocketMessage, WebSocketResult,
};
use url::Url;

use crate::request::build_url;
use crate::wasm::send_wrapper::SendWrapper;

fn map_send_error(error: GlooError) -> WebSocketError {
    match error {
        GlooError::MessageSendError(e) => WebSocketError::Protocol(e.to_string()),
        GlooError::ConnectionError => WebSocketError::Protocol("WebSocket connection error".into()),
        GlooError::ConnectionClose(e) => WebSocketError::Protocol(format!(
            "WebSocket closed: code {}, reason {}",
            e.code, e.reason
        )),
        _ => WebSocketError::Protocol("WebSocket send error".into()),
    }
}

fn map_open_error_to_invalid_url(_: impl std::fmt::Debug) -> WebSocketError {
    WebSocketError::InvalidUrl
}

fn build_ws_url(base_url: Option<&Url>, uri: &str) -> WebSocketResult<String> {
    let mut url = build_url(base_url, uri).map_err(|_| WebSocketError::InvalidUrl)?;
    match url.scheme() {
        "ws" | "wss" => {}
        "http" => {
            url.set_scheme("ws")
                .map_err(|_| WebSocketError::InvalidUrl)?;
        }
        "https" => {
            url.set_scheme("wss")
                .map_err(|_| WebSocketError::InvalidUrl)?;
        }
        _ => return Err(WebSocketError::InvalidUrl),
    }
    Ok(url.to_string())
}

fn create_sys_socket(
    url: &str,
    protocols: &[Cow<'static, str>],
) -> Result<web_sys::WebSocket, WebSocketError> {
    match protocols {
        [] => web_sys::WebSocket::new(url).map_err(map_open_error_to_invalid_url),
        [single] => {
            web_sys::WebSocket::new_with_str(url, single).map_err(map_open_error_to_invalid_url)
        }
        multiple => {
            let array = js_sys::Array::new();
            for protocol in multiple {
                array.push(&wasm_bindgen::JsValue::from_str(protocol));
            }
            let protocols_js: wasm_bindgen::JsValue = array.into();
            web_sys::WebSocket::new_with_str_sequence(url, &protocols_js)
                .map_err(map_open_error_to_invalid_url)
        }
    }
}

fn into_gloo_message(data: WebSocketMessage) -> Option<GlooMessage> {
    match data {
        WebSocketMessage::Utf8(data) => Some(GlooMessage::Text(data)),
        WebSocketMessage::Bytes(data) => Some(GlooMessage::Bytes(data)),
        WebSocketMessage::Close { .. } => None,
    }
}

fn from_gloo_message(
    message: GlooMessage,
    max_message_size: Option<u64>,
) -> WebSocketResult<WebSocketMessage> {
    match message {
        GlooMessage::Text(data) => {
            if let Some(max) = max_message_size {
                if data.len() as u64 > max {
                    return Err(WebSocketError::MessageTooLarge);
                }
            }
            Ok(WebSocketMessage::Utf8(data))
        }
        GlooMessage::Bytes(data) => {
            if let Some(max) = max_message_size {
                if data.len() as u64 > max {
                    return Err(WebSocketError::MessageTooLarge);
                }
            }
            Ok(WebSocketMessage::Bytes(data))
        }
    }
}

fn closed_error() -> WebSocketError {
    WebSocketError::Protocol("WebSocket sender is closed".into())
}

struct WasmSender {
    sink: SendWrapper<futures::stream::SplitSink<GlooWebSocket, GlooMessage>>,
    close_handle: SendWrapper<web_sys::WebSocket>,
    closing: bool,
    close_sent: bool,
}

struct WasmReceiver {
    source: SendWrapper<futures::stream::SplitStream<GlooWebSocket>>,
    max_message_size: Option<u64>,
    terminated: bool,
}

impl Sink<WebSocketMessage> for WasmSender {
    type Error = WebSocketError;

    fn poll_ready(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        let this = self.get_mut();
        if this.closing || this.close_sent {
            return Poll::Ready(Err(closed_error()));
        }
        Pin::new(&mut *this.sink)
            .poll_ready(cx)
            .map_err(map_send_error)
    }

    fn start_send(self: Pin<&mut Self>, item: WebSocketMessage) -> Result<(), Self::Error> {
        let this = self.get_mut();
        if this.closing || this.close_sent {
            return Err(closed_error());
        }
        if let WebSocketMessage::Close { code, reason } = item {
            this.close_handle
                .close_with_code_and_reason(code, &reason)
                .map_err(|_| WebSocketError::Protocol("WebSocket close failed".into()))?;
            this.close_sent = true;
            return Ok(());
        }
        let message =
            into_gloo_message(item).expect("non-close message always maps to gloo message");
        Pin::new(&mut *this.sink)
            .start_send(message)
            .map_err(map_send_error)
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Pin::new(&mut *self.get_mut().sink)
            .poll_flush(cx)
            .map_err(map_send_error)
    }

    fn poll_close(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        let this = self.get_mut();
        this.closing = true;
        if this.close_sent {
            return Pin::new(&mut *this.sink)
                .poll_flush(cx)
                .map_err(map_send_error);
        }
        // Browser validates the code; 1000 with empty reason is always valid.
        // Ignore failures here (e.g. already closing) to keep close idempotent,
        // matching the non-wasm backend's poll_close semantics.
        let _ = this.close_handle.close_with_code_and_reason(1000, "");
        this.close_sent = true;
        Pin::new(&mut *this.sink)
            .poll_flush(cx)
            .map_err(map_send_error)
    }
}

impl Stream for WasmReceiver {
    type Item = WebSocketResult<WebSocketMessage>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = self.get_mut();
        if this.terminated {
            return Poll::Ready(None);
        }
        match Pin::new(&mut *this.source).poll_next(cx) {
            Poll::Ready(None) => Poll::Ready(None),
            Poll::Ready(Some(Err(GlooError::ConnectionClose(event)))) => {
                this.terminated = true;
                Poll::Ready(Some(Ok(WebSocketMessage::Close {
                    code: event.code,
                    reason: event.reason,
                })))
            }
            Poll::Ready(Some(Err(GlooError::ConnectionError))) => {
                this.terminated = true;
                Poll::Ready(Some(Err(WebSocketError::Protocol(
                    "WebSocket connection error".into(),
                ))))
            }
            Poll::Ready(Some(Err(e))) => {
                this.terminated = true;
                Poll::Ready(Some(Err(map_send_error(e))))
            }
            Poll::Ready(Some(Ok(message))) => {
                match from_gloo_message(message, this.max_message_size) {
                    Ok(msg) => Poll::Ready(Some(Ok(msg))),
                    Err(e) => {
                        this.terminated = true;
                        Poll::Ready(Some(Err(e)))
                    }
                }
            }
            Poll::Pending => Poll::Pending,
        }
    }
}

pub(crate) async fn connect_async(
    base_url: Option<Url>,
    uri: Cow<'static, str>,
    protocols: Vec<Cow<'static, str>>,
    max_message_size: Option<u64>,
) -> WebSocketResult<nyquest_interface::r#async::websocket::WebSocket> {
    // NOTE: ClientOptions that have no browser WebSocket equivalent are intentionally
    // ignored here: request_timeout, caching_behavior, use_cookies, proxy_options,
    // follow_redirects and ignore_certificate_errors.
    let url = build_ws_url(base_url.as_ref(), &uri)?;
    let sys_socket = create_sys_socket(&url, &protocols)?;
    let close_handle = sys_socket.clone();
    let protocol_handle = sys_socket.clone();
    let mut gloo_socket =
        GlooWebSocket::try_from(sys_socket).map_err(|e| WebSocketError::Protocol(e.to_string()))?;

    // Wait until the browser finishes the opening handshake so that
    // `protocol()` reports the negotiated sub-protocol. This mirrors how the
    // non-wasm backend returns WebSocketInfo from the upgrade response.
    // poll_ready resolves once readyState leaves CONNECTING (OPEN/CLOSING/CLOSED);
    // handshake failures surface later via the receiver stream.
    let _ = futures::future::poll_fn(|cx| Pin::new(&mut gloo_socket).poll_ready(cx)).await;

    let protocol = protocol_handle.protocol();
    let info = WebSocketInfo {
        protocol: if protocol.is_empty() {
            None
        } else {
            Some(protocol)
        },
    };
    let (sink, source) = gloo_socket.split();
    Ok(nyquest_interface::r#async::websocket::WebSocket::new(
        info,
        nyquest_interface::r#async::websocket::WebSocketSender::from_impl(WasmSender {
            sink: SendWrapper::new(sink),
            close_handle: SendWrapper::new(close_handle),
            closing: false,
            close_sent: false,
        }),
        nyquest_interface::r#async::websocket::WebSocketReceiver::from_impl(WasmReceiver {
            source: SendWrapper::new(source),
            max_message_size,
            terminated: false,
        }),
    ))
}
