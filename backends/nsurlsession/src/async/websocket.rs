use std::borrow::Cow;
use std::future::poll_fn;
use std::sync::{Arc, Mutex};
use std::task::Poll;

use block2::RcBlock;
use futures_channel::oneshot;
use nyquest_interface::websocket::{WebSocketMessage, WebSocketResult};
use nyquest_interface::WebSocketError;
use objc2::{AllocAnyThread, DefinedClass};
use objc2_foundation::{
    NSData, NSString, NSURLSessionWebSocketCloseCode, NSURLSessionWebSocketMessage,
};

use crate::datatask::GenericWaker;
use crate::r#async::waker::AsyncWaker;
use crate::websocket::{
    check_message_size, create_task, decode_received, into_ws_info, ns_error_to_websocket, Socket,
    WebSocketTaskState,
};

impl Socket {
    pub(crate) fn send_async(
        &self,
        data: WebSocketMessage,
    ) -> oneshot::Receiver<WebSocketResult<()>> {
        let (tx, rx) = oneshot::channel();
        let message = match data {
            WebSocketMessage::Utf8(value) => NSURLSessionWebSocketMessage::initWithString(
                NSURLSessionWebSocketMessage::alloc(),
                &NSString::from_str(&value),
            ),
            WebSocketMessage::Bytes(value) => NSURLSessionWebSocketMessage::initWithData(
                NSURLSessionWebSocketMessage::alloc(),
                &NSData::from_vec(value),
            ),
            WebSocketMessage::Close { code, reason } => {
                self.task.cancelWithCloseCode_reason(
                    NSURLSessionWebSocketCloseCode(code as isize),
                    Some(&NSData::from_vec(reason.into_bytes())),
                );
                let _ = tx.send(Ok(()));
                return rx;
            }
        };
        let tx = Mutex::new(Some(tx));
        let callback = RcBlock::new(move |error: *mut objc2_foundation::NSError| {
            let result = unsafe {
                error
                    .as_ref()
                    .map_or(Ok(()), |e| Err(ns_error_to_websocket(e)))
            };
            if let Some(tx) = tx.lock().unwrap().take() {
                let _ = tx.send(result);
            }
        });
        unsafe {
            self.task.sendMessage_completionHandler(&message, &callback);
        }
        rx
    }

    pub(crate) fn receive_async(&self) -> oneshot::Receiver<WebSocketResult<WebSocketMessage>> {
        let (tx, rx) = oneshot::channel();
        let task = self.task.clone();
        let max_message_size = self.max_message_size;
        let tx = Mutex::new(Some(tx));
        let callback = RcBlock::new(
            move |message: *mut NSURLSessionWebSocketMessage,
                  error: *mut objc2_foundation::NSError| {
                let result = decode_received(&task, message, unsafe { error.as_ref() })
                    .and_then(|msg| check_message_size(msg, max_message_size));
                if let Some(tx) = tx.lock().unwrap().take() {
                    let _ = tx.send(result);
                }
            },
        );
        unsafe {
            self.task.receiveMessageWithCompletionHandler(&callback);
        }
        rx
    }
}

pub(super) async fn connect_async(
    client: crate::client::NSUrlSessionClient,
    uri: Cow<'static, str>,
    protocols: Vec<Cow<'static, str>>,
) -> WebSocketResult<nyquest_interface::r#async::websocket::WebSocket> {
    let waker = Arc::new(AsyncWaker::new());
    let (socket, delegate) =
        create_task(&client, uri, protocols, GenericWaker::Async(waker.clone()))?;
    let protocol = poll_fn(|cx| {
        waker.register(cx);
        let mut context = delegate.ivars().context.lock().unwrap();
        let is_connecting = matches!(context.state, WebSocketTaskState::Connecting);
        if is_connecting {
            Poll::Pending
        } else {
            Poll::Ready(context.protocol.take())
        }
    })
    .await;
    Ok(nyquest_interface::r#async::websocket::WebSocket::new(
        into_ws_info(protocol),
        nyquest_interface::r#async::websocket::WebSocketSender::from_impl(AsyncSender(
            socket.clone(),
        )),
        nyquest_interface::r#async::websocket::WebSocketReceiver::from_impl(AsyncReceiver(socket)),
    ))
}

struct AsyncSender(Arc<Socket>);

impl nyquest_interface::r#async::websocket::AsyncWebSocketSender for AsyncSender {
    async fn send(&mut self, data: WebSocketMessage) -> WebSocketResult<()> {
        self.0
            .send_async(data)
            .await
            .map_err(|_| WebSocketError::Protocol("WebSocket send callback stopped".into()))?
    }

    async fn close(&mut self) -> WebSocketResult<()> {
        self.send(WebSocketMessage::Close {
            code: 1000,
            reason: String::new(),
        })
        .await
    }
}

struct AsyncReceiver(Arc<Socket>);

impl nyquest_interface::r#async::websocket::AsyncWebSocketReceiver for AsyncReceiver {
    async fn receive(&mut self) -> WebSocketResult<WebSocketMessage> {
        self.0
            .receive_async()
            .await
            .map_err(|_| WebSocketError::Protocol("WebSocket receive callback stopped".into()))?
    }
}
