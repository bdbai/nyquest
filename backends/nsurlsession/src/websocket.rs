#![allow(non_snake_case)]

use std::borrow::Cow;
use std::io;
use std::sync::{Arc, Mutex};

use nyquest_interface::websocket::{
    WebSocketError, WebSocketInfo, WebSocketMessage, WebSocketResult,
};
use objc2::rc::{autoreleasepool, Retained};
use objc2::runtime::ProtocolObject;
use objc2::{define_class, msg_send, AllocAnyThread, DefinedClass};
use objc2_foundation::{
    NSError, NSObject, NSObjectProtocol, NSString, NSURLSession, NSURLSessionDelegate,
    NSURLSessionTask, NSURLSessionTaskDelegate, NSURLSessionWebSocketDelegate,
    NSURLSessionWebSocketMessage, NSURLSessionWebSocketMessageType, NSURLSessionWebSocketTask,
    NSURL,
};

use crate::datatask::GenericWaker;

pub(crate) struct Socket {
    pub(crate) task: Retained<NSURLSessionWebSocketTask>,
    pub(crate) max_message_size: usize,
}

impl Drop for Socket {
    fn drop(&mut self) {
        self.task.cancel();
    }
}

pub(crate) struct WebSocketTaskContext {
    pub(crate) protocol: Option<String>,
    pub(crate) state: WebSocketTaskState,
}

pub(crate) enum WebSocketTaskState {
    Connecting,
    Open,
    Closed,
}

pub(crate) struct WebSocketTaskIvars {
    pub(crate) context: Mutex<WebSocketTaskContext>,
    pub(crate) waker: GenericWaker,
}

define_class!(
    #[unsafe(super = NSObject)]
    #[name = "Nyquest_WebSocketTaskDelegate"]
    #[ivars = WebSocketTaskIvars]
    pub(crate) struct WebSocketTaskDelegate;

    unsafe impl NSObjectProtocol for WebSocketTaskDelegate {}

    unsafe impl NSURLSessionDelegate for WebSocketTaskDelegate {}

    unsafe impl NSURLSessionTaskDelegate for WebSocketTaskDelegate {
        #[unsafe(method(URLSession:task:didCompleteWithError:))]
        fn URLSession_task_didCompleteWithError(
            &self,
            _session: &NSURLSession,
            _task: &NSURLSessionTask,
            error: Option<&NSError>,
        ) {
            // TODO: handle this error
            let ivars = self.ivars();
            let mut ctx = ivars.context.lock().unwrap();
            ctx.state = WebSocketTaskState::Closed;
            ivars.waker.wake();
        }
    }

    unsafe impl NSURLSessionWebSocketDelegate for WebSocketTaskDelegate {
        #[unsafe(method(URLSession:webSocketTask:didOpenWithProtocol:))]
        fn URLSession_webSocketTask_didOpenWithProtocol(
            &self,
            _session: &NSURLSession,
            _web_socket_task: &NSURLSessionWebSocketTask,
            protocol: Option<&NSString>,
        ) {
            let ivars = self.ivars();
            let mut ctx = ivars.context.lock().unwrap();
            ctx.protocol = protocol.map(|p| p.to_string()).filter(|p| !p.is_empty());
            ctx.state = WebSocketTaskState::Open;
            ivars.waker.wake();
        }
    }
);

impl WebSocketTaskDelegate {
    pub(crate) fn new(waker: GenericWaker) -> Retained<Self> {
        let this = Self::alloc().set_ivars(WebSocketTaskIvars {
            context: Mutex::new(WebSocketTaskContext {
                state: WebSocketTaskState::Connecting,
                protocol: None,
            }),
            waker,
        });
        unsafe { msg_send![super(this), init] }
    }
}

/// Converts an incoming callback message into a `WebSocketMessage`, checking close state.
pub(crate) fn decode_received(
    task: &NSURLSessionWebSocketTask,
    message: *mut NSURLSessionWebSocketMessage,
    error: Option<&objc2_foundation::NSError>,
) -> WebSocketResult<WebSocketMessage> {
    if message.is_null() {
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
        } else if let Some(error) = error {
            Err(ns_error_to_websocket(error))
        } else {
            Err(WebSocketError::Protocol(
                "WebSocket closed without a close frame".into(),
            ))
        }
    } else {
        let message = unsafe { &*message };
        match message.r#type() {
            NSURLSessionWebSocketMessageType::String => message
                .string()
                .map(|value| WebSocketMessage::Utf8(value.to_string()))
                .ok_or_else(|| WebSocketError::Protocol("WebSocket text message missing".into())),
            NSURLSessionWebSocketMessageType::Data => message
                .data()
                .map(|value| WebSocketMessage::Bytes(value.to_vec()))
                .ok_or_else(|| WebSocketError::Protocol("WebSocket binary message missing".into())),
            _ => Err(WebSocketError::Protocol(
                "Unknown WebSocket message type".into(),
            )),
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

pub(crate) fn create_task(
    client: &crate::client::NSUrlSessionClient,
    uri: Cow<'static, str>,
    protocols: Vec<Cow<'static, str>>,
    waker: GenericWaker,
) -> WebSocketResult<(Arc<Socket>, Retained<WebSocketTaskDelegate>)> {
    let url =
        NSURL::URLWithString_relativeToURL(&NSString::from_str(&uri), client.base_url.as_deref())
            .ok_or(WebSocketError::InvalidUrl)?;
    let scheme = url.scheme().map(|s| s.to_string()).unwrap_or_default();
    if !scheme.eq_ignore_ascii_case("ws") && !scheme.eq_ignore_ascii_case("wss") {
        return Err(WebSocketError::InvalidUrl);
    }
    let task = if !protocols.is_empty() {
        let ns_protocols: Vec<_> = protocols.iter().map(|p| NSString::from_str(p)).collect();
        let protocols_array = objc2_foundation::NSArray::from_retained_slice(&ns_protocols);
        client
            .session
            .webSocketTaskWithURL_protocols(&url, &protocols_array)
    } else {
        client.session.webSocketTaskWithURL(&url)
    };
    task.setMaximumMessageSize(client.max_response_buffer_size as isize);
    let delegate = WebSocketTaskDelegate::new(waker);
    task.setDelegate(Some(ProtocolObject::from_ref(&*delegate)));
    task.resume();
    let socket = Arc::new(Socket {
        task,
        max_message_size: usize::try_from(client.max_response_buffer_size).unwrap_or(usize::MAX),
    });
    Ok((socket, delegate))
}

pub(crate) fn into_ws_info(protocol: Option<String>) -> WebSocketInfo {
    WebSocketInfo { protocol }
}

pub(crate) fn ns_error_to_websocket(error: &NSError) -> WebSocketError {
    let msg =
        autoreleasepool(|pool| unsafe { (error).localizedDescription().to_str(pool).to_owned() });
    WebSocketError::Io(io::Error::other(format!(
        "NSURLSession error {}: {}",
        error.code(),
        msg
    )))
}
