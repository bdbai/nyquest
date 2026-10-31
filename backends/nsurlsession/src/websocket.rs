use std::borrow::Cow;
use std::sync::Arc;

use nyquest_interface::websocket::{WebSocketError, WebSocketMessage, WebSocketResult};
use objc2::rc::Retained;
use objc2_foundation::{NSString, NSURLSessionWebSocketMessage, NSURLSessionWebSocketTask, NSURL};

pub(crate) struct Socket {
    pub(crate) task: Retained<NSURLSessionWebSocketTask>,
    pub(crate) max_message_size: usize,
}

unsafe impl Send for Socket {}
unsafe impl Sync for Socket {}

impl Drop for Socket {
    fn drop(&mut self) {
        self.task.cancel();
    }
}

/// Converts an incoming callback message into a `WebSocketMessage`, checking close state.
pub(crate) fn decode_received(
    task: &NSURLSessionWebSocketTask,
    message: *mut NSURLSessionWebSocketMessage,
    error: *mut objc2_foundation::NSError,
) -> WebSocketResult<WebSocketMessage> {
    if !message.is_null() {
        let message = unsafe { &*message };
        match message.r#type() {
            objc2_foundation::NSURLSessionWebSocketMessageType::String => message
                .string()
                .map(|value| WebSocketMessage::Utf8(value.to_string()))
                .ok_or_else(|| WebSocketError::Protocol("WebSocket text message missing".into())),
            objc2_foundation::NSURLSessionWebSocketMessageType::Data => message
                .data()
                .map(|value| WebSocketMessage::Bytes(value.to_vec()))
                .ok_or_else(|| WebSocketError::Protocol("WebSocket binary message missing".into())),
            _ => Err(WebSocketError::Protocol(
                "Unknown WebSocket message type".into(),
            )),
        }
    } else {
        let code = task.closeCode().0;
        if code != 0 {
            let reason = task
                .closeReason()
                .map(|value| String::from_utf8_lossy(&value.to_vec()).into_owned())
                .unwrap_or_default();
            Ok(WebSocketMessage::Close {
                code: code as u16,
                reason,
            })
        } else if error.is_null() {
            Err(WebSocketError::Protocol(
                "WebSocket closed without a close frame".into(),
            ))
        } else {
            Err(WebSocketError::Protocol("WebSocket receive failed".into()))
        }
    }
}

/// Enforces the configured max message size limit.
pub(crate) fn check_message_size(
    message: WebSocketMessage,
    max_message_size: usize,
) -> WebSocketResult<WebSocketMessage> {
    let size = match &message {
        WebSocketMessage::Utf8(value) => value.len(),
        WebSocketMessage::Bytes(value) => value.len(),
        WebSocketMessage::Close { reason, .. } => reason.len(),
    };
    if size > max_message_size {
        Err(WebSocketError::MessageTooLarge)
    } else {
        Ok(message)
    }
}

pub(crate) fn connect(
    client: &crate::client::NSUrlSessionClient,
    uri: Cow<'static, str>,
    protocol: Option<Cow<'static, str>>,
) -> WebSocketResult<Arc<Socket>> {
    let url =
        NSURL::URLWithString_relativeToURL(&NSString::from_str(&uri), client.base_url.as_deref())
            .ok_or(WebSocketError::InvalidUrl)?;
    let scheme = url.scheme().map(|s| s.to_string()).unwrap_or_default();
    if !scheme.eq_ignore_ascii_case("ws") && !scheme.eq_ignore_ascii_case("wss") {
        return Err(WebSocketError::InvalidUrl);
    }
    let task = if let Some(protocol) = protocol {
        let protocols =
            objc2_foundation::NSArray::from_retained_slice(&[NSString::from_str(&protocol)]);
        client
            .session
            .webSocketTaskWithURL_protocols(&url, &protocols)
    } else {
        client.session.webSocketTaskWithURL(&url)
    };
    task.setMaximumMessageSize(client.max_response_buffer_size as isize);
    task.resume();
    Ok(Arc::new(Socket {
        task,
        max_message_size: usize::try_from(client.max_response_buffer_size).unwrap_or(usize::MAX),
    }))
}
