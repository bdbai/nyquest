use std::borrow::Cow;
use std::future::poll_fn;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};

use block2::RcBlock;
use futures_channel::oneshot;
use futures_core::Stream;
use futures_sink::Sink;
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
        match context.take_state() {
            WebSocketTaskState::Connecting => Poll::Pending,
            WebSocketTaskState::Open => Poll::Ready(Ok(context.protocol.take())),
            WebSocketTaskState::Closed(error) => Poll::Ready(Err(error)),
        }
    })
    .await?;
    Ok(nyquest_interface::r#async::websocket::WebSocket::new(
        into_ws_info(protocol),
        nyquest_interface::r#async::websocket::WebSocketSender::from_impl(AsyncSender {
            socket: socket.clone(),
            pending: None,
            closing: false,
            close_sent: false,
        }),
        nyquest_interface::r#async::websocket::WebSocketReceiver::from_impl(AsyncReceiver {
            socket,
            pending_recv: None,
            terminated: false,
        }),
    ))
}

struct AsyncSender {
    socket: Arc<Socket>,
    pending: Option<Pin<Box<oneshot::Receiver<WebSocketResult<()>>>>>,
    closing: bool,
    close_sent: bool,
}

fn closed_error() -> WebSocketError {
    WebSocketError::Protocol("WebSocket sender is closed".into())
}

impl AsyncSender {
    fn poll_pending(&mut self, cx: &mut Context<'_>) -> Poll<WebSocketResult<()>> {
        let Some(rx) = self.pending.as_mut() else {
            return Poll::Ready(Ok(()));
        };
        match rx.as_mut().poll(cx) {
            Poll::Ready(Ok(Ok(()))) => {
                self.pending = None;
                Poll::Ready(Ok(()))
            }
            Poll::Ready(Ok(Err(e))) => {
                self.pending = None;
                Poll::Ready(Err(e))
            }
            Poll::Ready(Err(_canceled)) => {
                self.pending = None;
                Poll::Ready(Err(WebSocketError::Protocol(
                    "WebSocket send callback stopped".into(),
                )))
            }
            Poll::Pending => Poll::Pending,
        }
    }
}

impl Sink<WebSocketMessage> for AsyncSender {
    type Error = WebSocketError;

    fn poll_ready(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        // All fields are `Unpin` (futures are boxed), so pinning is trivial.
        let this = self.get_mut();
        if this.closing || this.close_sent {
            return Poll::Ready(Err(closed_error()));
        }
        if this.pending.is_some() {
            match this.poll_pending(cx) {
                Poll::Ready(Ok(())) => Poll::Ready(Ok(())),
                Poll::Ready(Err(e)) => Poll::Ready(Err(e)),
                Poll::Pending => Poll::Pending,
            }
        } else {
            Poll::Ready(Ok(()))
        }
    }

    fn start_send(self: Pin<&mut Self>, item: WebSocketMessage) -> Result<(), Self::Error> {
        let this = self.get_mut();
        if this.closing || this.close_sent {
            return Err(closed_error());
        }
        if this.pending.is_some() {
            return Err(WebSocketError::Protocol(
                "WebSocket send buffer is full".into(),
            ));
        }
        let close = matches!(item, WebSocketMessage::Close { .. });
        this.pending = Some(Box::pin(this.socket.send_async(item)));
        if close {
            this.close_sent = true;
        }
        Ok(())
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        let this = self.get_mut();
        this.poll_pending(cx)
    }

    fn poll_close(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        let this = self.get_mut();
        this.closing = true;
        // A close frame sent via `start_send` already initiates the handshake;
        // only flush it instead of emitting a second one.
        if this.close_sent {
            return this.poll_pending(cx);
        }
        // Flush any in-flight message first.
        match this.poll_pending(cx) {
            Poll::Pending => return Poll::Pending,
            Poll::Ready(Err(e)) => return Poll::Ready(Err(e)),
            Poll::Ready(Ok(())) => {}
        }
        // `send_async` with Close completes synchronously (already-resolved channel),
        // but poll it once for uniformity.
        this.pending = Some(Box::pin(this.socket.send_async(WebSocketMessage::Close {
            code: 1000,
            reason: String::new(),
        })));
        this.close_sent = true;
        this.poll_pending(cx)
    }
}

struct AsyncReceiver {
    socket: Arc<Socket>,
    pending_recv: Option<Pin<Box<oneshot::Receiver<WebSocketResult<WebSocketMessage>>>>>,
    /// Set once a close frame or a fatal error has been yielded;
    /// subsequent `poll_next` calls return `None`.
    terminated: bool,
}

impl Stream for AsyncReceiver {
    type Item = WebSocketResult<WebSocketMessage>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = self.get_mut();
        if this.terminated {
            return Poll::Ready(None);
        }
        let rx = this
            .pending_recv
            .get_or_insert_with(|| Box::pin(this.socket.receive_async()));
        match rx.as_mut().poll(cx) {
            Poll::Ready(Ok(Ok(msg))) => {
                this.pending_recv = None;
                if matches!(msg, WebSocketMessage::Close { .. }) {
                    this.terminated = true;
                }
                Poll::Ready(Some(Ok(msg)))
            }
            Poll::Ready(Ok(Err(e))) => {
                this.pending_recv = None;
                // `MessageTooLarge` is recoverable (see above); any other
                // receive failure terminates the stream after this item.
                if !matches!(e, WebSocketError::MessageTooLarge) {
                    this.terminated = true;
                }
                Poll::Ready(Some(Err(e)))
            }
            Poll::Ready(Err(_canceled)) => {
                this.pending_recv = None;
                this.terminated = true;
                Poll::Ready(Some(Err(WebSocketError::Protocol(
                    "WebSocket receive callback stopped".into(),
                ))))
            }
            Poll::Pending => Poll::Pending,
        }
    }
}
