//! Blocking WebSocket support.

use std::fmt;

use crate::{
    blocking::any::{AnyBlockingWebSocketReceiver, AnyBlockingWebSocketSender},
    websocket::{WebSocketInfo, WebSocketMessage, WebSocketResult},
};

/// A blocking WebSocket connection.
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

/// The sending half of a blocking WebSocket.
pub struct WebSocketSender {
    pub(crate) inner: Box<dyn AnyBlockingWebSocketSender>,
}

impl WebSocketSender {
    /// Creates a sender from a backend implementation.
    pub fn from_impl<T: BlockingWebSocketSender>(sender: T) -> Self {
        Self {
            inner: Box::new(sender),
        }
    }
    /// Sends a message.
    pub fn send(&mut self, data: WebSocketMessage) -> WebSocketResult<()> {
        self.inner.send(data)
    }
    /// Closes the connection.
    pub fn close(&mut self) -> WebSocketResult<()> {
        self.inner.close()
    }
}

/// The receiving half of a blocking WebSocket.
pub struct WebSocketReceiver {
    pub(crate) inner: Box<dyn AnyBlockingWebSocketReceiver>,
}

/// Backend implementation of a blocking WebSocket sender.
pub trait BlockingWebSocketSender: Send + 'static {
    /// Sends a message.
    fn send(&mut self, data: WebSocketMessage) -> WebSocketResult<()>;
    /// Closes the connection.
    fn close(&mut self) -> WebSocketResult<()>;
}

/// Backend implementation of a blocking WebSocket receiver.
pub trait BlockingWebSocketReceiver: Send + 'static {
    /// Receives the next message.
    fn receive(&mut self) -> WebSocketResult<WebSocketMessage>;
}

impl WebSocketReceiver {
    /// Creates a receiver from a backend implementation.
    pub fn from_impl<T: BlockingWebSocketReceiver>(receiver: T) -> Self {
        Self {
            inner: Box::new(receiver),
        }
    }
    /// Receives the next message.
    pub fn receive(&mut self) -> WebSocketResult<WebSocketMessage> {
        self.inner.receive()
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

impl<T: BlockingWebSocketSender> AnyBlockingWebSocketSender for T {
    fn send(&mut self, data: WebSocketMessage) -> WebSocketResult<()> {
        BlockingWebSocketSender::send(self, data)
    }
    fn close(&mut self) -> WebSocketResult<()> {
        BlockingWebSocketSender::close(self)
    }
}

impl<T: BlockingWebSocketReceiver> AnyBlockingWebSocketReceiver for T {
    fn receive(&mut self) -> WebSocketResult<WebSocketMessage> {
        BlockingWebSocketReceiver::receive(self)
    }
}
