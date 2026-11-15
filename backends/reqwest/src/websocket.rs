use std::borrow::Cow;

use futures::StreamExt;
use nyquest_interface::websocket::{
    WebSocketError, WebSocketInfo, WebSocketMessage, WebSocketResult,
};
use tokio_tungstenite::{
    tungstenite::{
        protocol::{frame::coding::CloseCode, CloseFrame, Role, WebSocketConfig},
        Message,
    },
    WebSocketStream,
};
use url::Url;

use crate::request::build_url;

pub(crate) type Stream = WebSocketStream<reqwest::Upgraded>;
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

fn map_reqwest_error(error: reqwest::Error) -> WebSocketError {
    if error.is_builder() {
        WebSocketError::InvalidUrl
    } else if error.is_timeout() {
        WebSocketError::Io(std::io::Error::new(std::io::ErrorKind::TimedOut, error))
    } else {
        WebSocketError::Io(std::io::Error::other(error))
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
    http_client: reqwest::Client,
    base_url: Option<&Url>,
    uri: Cow<'static, str>,
    protocols: Vec<Cow<'static, str>>,
    max_message_size: Option<u64>,
) -> WebSocketResult<(WebSocketInfo, Sink, Source)> {
    let mut url = build_url(base_url, &uri).map_err(|_| WebSocketError::InvalidUrl)?;
    let http_scheme = match url.scheme() {
        "ws" | "http" => "http",
        "wss" | "https" => "https",
        _ => return Err(WebSocketError::InvalidUrl),
    };
    url.set_scheme(http_scheme)
        .map_err(|_| WebSocketError::InvalidUrl)?;

    let sec_key = tokio_tungstenite::tungstenite::handshake::client::generate_key();
    let mut req = http_client
        .get(url)
        .version(http::Version::HTTP_11)
        .header(http::header::CONNECTION, "Upgrade")
        .header(http::header::UPGRADE, "websocket")
        .header(http::header::SEC_WEBSOCKET_VERSION, "13")
        .header(http::header::SEC_WEBSOCKET_KEY, sec_key);
    if !protocols.is_empty() {
        let joined = protocols.join(", ");
        let value = http::HeaderValue::from_str(&joined).map_err(|_| WebSocketError::InvalidUrl)?;
        req = req.header(http::header::SEC_WEBSOCKET_PROTOCOL, value);
    }

    let response = req.send().await.map_err(map_reqwest_error)?;
    if response.status() != http::StatusCode::SWITCHING_PROTOCOLS {
        return Err(WebSocketError::Protocol(format!(
            "WebSocket upgrade failed: HTTP status {}",
            response.status()
        )));
    }
    let protocol = response
        .headers()
        .get(http::header::SEC_WEBSOCKET_PROTOCOL)
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned);
    let upgraded = response.upgrade().await.map_err(map_reqwest_error)?;

    let mut config = WebSocketConfig::default();
    let max_size = max_message_size.and_then(|size| usize::try_from(size).ok());
    config.max_message_size = max_size;
    config.max_frame_size = max_size;
    let stream = WebSocketStream::from_raw_socket(upgraded, Role::Client, Some(config)).await;
    let (sink, source) = stream.split();
    Ok((WebSocketInfo { protocol }, sink, source))
}
