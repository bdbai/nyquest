use std::borrow::Cow;
use std::future::{poll_fn, Future};
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};

use block2::RcBlock;
use futures_util::task::AtomicWaker;
use nyquest_interface::websocket::{WebSocketError, WebSocketMessage, WebSocketResult};
use objc2::{AllocAnyThread, DefinedClass};
use objc2_foundation::{
    NSData, NSString, NSURLSessionWebSocketCloseCode, NSURLSessionWebSocketMessage,
};

use crate::datatask::GenericWaker;
use crate::r#async::waker::AsyncWaker;
use crate::websocket::{check_message_size, create_task, decode_received, into_ws_info, Socket};

struct AsyncState<T> {
    result: Mutex<Option<WebSocketResult<T>>>,
    waker: AtomicWaker,
}

impl<T> AsyncState<T> {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            result: Mutex::new(None),
            waker: AtomicWaker::new(),
        })
    }

    fn complete(&self, result: WebSocketResult<T>) {
        *self.result.lock().unwrap() = Some(result);
        self.waker.wake();
    }
}

pub(crate) struct AsyncOperation<T> {
    state: Arc<AsyncState<T>>,
}

impl<T> Future for AsyncOperation<T> {
    type Output = WebSocketResult<T>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        if let Some(result) = self.state.result.lock().unwrap().take() {
            return Poll::Ready(result);
        }
        self.state.waker.register(cx.waker());
        if let Some(result) = self.state.result.lock().unwrap().take() {
            Poll::Ready(result)
        } else {
            Poll::Pending
        }
    }
}

impl Socket {
    pub(crate) fn send_async(&self, data: WebSocketMessage) -> AsyncOperation<()> {
        let state = AsyncState::new();
        let callback_state = state.clone();
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
                state.complete(Ok(()));
                return AsyncOperation { state };
            }
        };
        let callback = RcBlock::new(move |error: *mut objc2_foundation::NSError| {
            callback_state.complete(if error.is_null() {
                Ok(())
            } else {
                Err(WebSocketError::Protocol("WebSocket send failed".into()))
            });
        });
        unsafe {
            self.task.sendMessage_completionHandler(&message, &callback);
        }
        AsyncOperation { state }
    }

    pub(crate) fn receive_async(&self) -> AsyncOperation<WebSocketMessage> {
        let state = AsyncState::new();
        let callback_state = state.clone();
        let task = self.task.clone();
        let max_message_size = self.max_message_size;
        let callback = RcBlock::new(
            move |message: *mut NSURLSessionWebSocketMessage,
                  error: *mut objc2_foundation::NSError| {
                let result = decode_received(&task, message, error)
                    .and_then(|msg| check_message_size(msg, max_message_size));
                callback_state.complete(result);
            },
        );
        unsafe {
            self.task.receiveMessageWithCompletionHandler(&callback);
        }
        AsyncOperation { state }
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
        if let Some(res) = delegate.ivars().result.lock().unwrap().take() {
            return Poll::Ready(res);
        }
        waker.register(cx);
        if let Some(res) = delegate.ivars().result.lock().unwrap().take() {
            Poll::Ready(res)
        } else {
            Poll::Pending
        }
    })
    .await?;
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
        self.0.send_async(data).await
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
        self.0.receive_async().await
    }
}
