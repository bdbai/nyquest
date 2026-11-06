use std::borrow::Cow;

use futures::StreamExt;
use nyquest_interface::websocket::{
    WebSocketError, WebSocketInfo, WebSocketMessage, WebSocketResult,
};
use tokio_tungstenite::{
    connect_async_with_config,
    tungstenite::{
        client::IntoClientRequest,
        protocol::{frame::coding::CloseCode, CloseFrame, WebSocketConfig},
        Message,
    },
    MaybeTlsStream, WebSocketStream,
};
use url::Url;

use crate::request::build_url;

pub(crate) type Stream = WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>;
pub(crate) type Sink = futures::stream::SplitSink<Stream, Message>;
pub(crate) type Source = futures::stream::SplitStream<Stream>;

pub(crate) fn map_error(error: tokio_tungstenite::tungstenite::Error) -> WebSocketError {
    match error {
        tokio_tungstenite::tungstenite::Error::Io(error) => WebSocketError::Io(error),
        tokio_tungstenite::tungstenite::Error::Capacity(_) => WebSocketError::MessageTooLarge,
        tokio_tungstenite::tungstenite::Error::Url(_) => WebSocketError::InvalidUrl,
        error => WebSocketError::Protocol(error.to_string()),
    }
}

pub(crate) fn into_tungstenite_message(data: WebSocketMessage) -> Message {
    match data {
        WebSocketMessage::Utf8(data) => Message::Text(data.into()),
        WebSocketMessage::Bytes(data) => Message::Binary(data.into()),
        WebSocketMessage::Close { code, reason } => Message::Close(Some(CloseFrame {
            code: CloseCode::from(code),
            reason: reason.into(),
        })),
    }
}

pub(crate) fn from_tungstenite_message(message: Message) -> Option<WebSocketMessage> {
    match message {
        Message::Text(data) => Some(WebSocketMessage::Utf8(data.to_string())),
        Message::Binary(data) => Some(WebSocketMessage::Bytes(data.to_vec())),
        Message::Close(frame) => {
            let (code, reason) = frame
                .map(|frame| (u16::from(frame.code), frame.reason.to_string()))
                .unwrap_or((1005, String::new()));
            Some(WebSocketMessage::Close { code, reason })
        }
        Message::Ping(_) | Message::Pong(_) | Message::Frame(_) => None,
    }
}

pub(crate) async fn connect_parts(
    base_url: Option<&Url>,
    uri: Cow<'static, str>,
    protocols: Vec<Cow<'static, str>>,
    max_message_size: Option<u64>,
) -> WebSocketResult<(WebSocketInfo, Sink, Source)> {
    let url = build_url(base_url, &uri).map_err(|_| WebSocketError::InvalidUrl)?;
    if !matches!(url.scheme(), "ws" | "wss") {
        return Err(WebSocketError::InvalidUrl);
    }
    let mut request = url
        .as_str()
        .into_client_request()
        .map_err(|_| WebSocketError::InvalidUrl)?;
    if !protocols.is_empty() {
        let joined = protocols.join(", ");
        let value = http::HeaderValue::from_str(&joined).map_err(|_| WebSocketError::InvalidUrl)?;
        request
            .headers_mut()
            .insert("Sec-WebSocket-Protocol", value);
    }

    let mut config = WebSocketConfig::default();
    let max_size = max_message_size.and_then(|size| usize::try_from(size).ok());
    config.max_message_size = max_size;
    config.max_frame_size = max_size;
    let (stream, response) = connect_async_with_config(request, Some(config), false)
        .await
        .map_err(map_error)?;
    let protocol = response
        .headers()
        .get("Sec-WebSocket-Protocol")
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned);
    let (sink, source) = stream.split();
    Ok((WebSocketInfo { protocol }, sink, source))
}
