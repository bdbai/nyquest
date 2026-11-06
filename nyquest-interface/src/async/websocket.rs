//! Asynchronous WebSocket support.

use std::fmt;
use std::future::Future;

use futures_core::future::BoxFuture;

use crate::r#async::any::{AnyAsyncWebSocketReceiver, AnyAsyncWebSocketSender};
use crate::websocket::{WebSocketInfo, WebSocketMessage, WebSocketResult};

/// An asynchronous WebSocket connection.
#[derive(Debug)]
pub struct WebSocket {
    /// Information about the connection.
    pub info: WebSocketInfo,
    /// The sending half of the connection.
    pub sender: WebSocketSender,
    /// The receiving half of the connection.
    pub receiver: WebSocketReceiver,
}

impl WebSocket {
    /// Creates a connection from backend-provided halves.
    pub fn new(info: WebSocketInfo, sender: WebSocketSender, receiver: WebSocketReceiver) -> Self {
        Self {
            info,
            sender,
            receiver,
        }
    }
}

/// The sending half of an asynchronous WebSocket.
pub struct WebSocketSender {
    pub(crate) inner: Box<dyn AnyAsyncWebSocketSender>,
}

/// The receiving half of an asynchronous WebSocket.
pub struct WebSocketReceiver {
    pub(crate) inner: Box<dyn AnyAsyncWebSocketReceiver>,
}

/// Backend implementation of an asynchronous WebSocket receiver.
pub trait AsyncWebSocketReceiver: Send + 'static {
    /// Receives the next message.
    fn receive(&mut self) -> impl Future<Output = WebSocketResult<WebSocketMessage>> + Send;
}

/// Backend implementation of an asynchronous WebSocket sender.
pub trait AsyncWebSocketSender: Send + 'static {
    /// Sends a message.
    fn send(&mut self, data: WebSocketMessage) -> impl Future<Output = WebSocketResult<()>> + Send;
    /// Closes the connection.
    fn close(&mut self) -> impl Future<Output = WebSocketResult<()>> + Send;
}

impl WebSocketSender {
    /// Creates a sender from a backend implementation.
    pub fn from_impl<T: AsyncWebSocketSender>(sender: T) -> Self {
        Self {
            inner: Box::new(sender),
        }
    }

    /// Sends a message.
    pub async fn send(&mut self, data: WebSocketMessage) -> WebSocketResult<()> {
        self.inner.send(data).await
    }

    /// Closes the connection.
    pub async fn close(&mut self) -> WebSocketResult<()> {
        self.inner.close().await
    }
}

impl WebSocketReceiver {
    /// Creates a receiver from a backend implementation.
    pub fn from_impl<T: AsyncWebSocketReceiver>(receiver: T) -> Self {
        Self {
            inner: Box::new(receiver),
        }
    }

    /// Receives the next message.
    pub async fn receive(&mut self) -> WebSocketResult<WebSocketMessage> {
        self.inner.receive().await
    }
}

impl fmt::Debug for WebSocketSender {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "WebSocketSender")
    }
}

impl fmt::Debug for WebSocketReceiver {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "WebSocketReceiver")
    }
}

impl<T: AsyncWebSocketSender> AnyAsyncWebSocketSender for T {
    fn send(&mut self, data: WebSocketMessage) -> BoxFuture<'_, WebSocketResult<()>> {
        Box::pin(AsyncWebSocketSender::send(self, data))
    }
    fn close(&mut self) -> BoxFuture<'_, WebSocketResult<()>> {
        Box::pin(AsyncWebSocketSender::close(self))
    }
}

impl<T: AsyncWebSocketReceiver> AnyAsyncWebSocketReceiver for T {
    fn receive(&mut self) -> BoxFuture<'_, WebSocketResult<WebSocketMessage>> {
        Box::pin(AsyncWebSocketReceiver::receive(self))
    }
}
