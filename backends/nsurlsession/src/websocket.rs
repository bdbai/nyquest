#![allow(non_snake_case)]

use std::borrow::Cow;
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
    NSURLSessionWebSocketMessage, NSURLSessionWebSocketTask, NSURL,
};

use crate::datatask::GenericWaker;

pub(crate) struct Socket {
    pub(crate) task: Retained<NSURLSessionWebSocketTask>,
    pub(crate) max_message_size: usize,
    _delegate: Retained<WebSocketTaskDelegate>,
}

unsafe impl Send for Socket {}
unsafe impl Sync for Socket {}

impl Drop for Socket {
    fn drop(&mut self) {
        self.task.cancel();
    }
}

pub(crate) struct WebSocketTaskIvars {
    pub(crate) result: Mutex<Option<WebSocketResult<Option<String>>>>,
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
            let ivars = self.ivars();
            let mut slot = ivars.result.lock().unwrap();
            if slot.is_none() {
                let err = if let Some(error) = error {
                    let msg = autoreleasepool(|pool| unsafe {
                        error.localizedDescription().to_str(pool).to_owned()
                    });
                    WebSocketError::Io(std::io::Error::other(format!(
                        "NSURLSession error {}: {msg}",
                        error.code()
                    )))
                } else {
                    WebSocketError::Protocol("WebSocket closed before handshake completed".into())
                };
                *slot = Some(Err(err));
                drop(slot);
                ivars.waker.wake();
            }
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
            let mut slot = ivars.result.lock().unwrap();
            if slot.is_none() {
                let protocol = protocol.map(|p| p.to_string()).filter(|p| !p.is_empty());
                *slot = Some(Ok(protocol));
                drop(slot);
                ivars.waker.wake();
            }
        }
    }
);

impl WebSocketTaskDelegate {
    pub(crate) fn new(waker: GenericWaker) -> Retained<Self> {
        let this = Self::alloc().set_ivars(WebSocketTaskIvars {
            result: Mutex::new(None),
            waker,
        });
        unsafe { msg_send![super(this), init] }
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
        _delegate: delegate.clone(),
    });
    Ok((socket, delegate))
}

pub(crate) fn into_ws_info(protocol: Option<String>) -> WebSocketInfo {
    WebSocketInfo { protocol }
}
