//! Asynchronous WebSocket support.

use std::fmt;
use std::pin::Pin;
use std::task::{Context, Poll};

use futures_core::Stream;
use futures_sink::Sink;

use crate::r#async::any::{AnyAsyncWebSocketReceiver, AnyAsyncWebSocketSender};
use crate::websocket::{WebSocketError, WebSocketInfo, WebSocketMessage, WebSocketResult};

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
///
/// Implements [`Sink`] with `Item = WebSocketMessage` and `Error = WebSocketError`.
/// Use `SinkExt` (e.g. via `futures-util`) to `send` messages and
/// `close` the connection.
///
/// ## Close semantics
///
/// At most one close frame is ever emitted on the wire, no matter how it is
/// initiated:
///
/// - Sending a [`WebSocketMessage::Close`] item through the sink starts the close
///   handshake. Further `start_send` calls (with any message, including another
///   close) fail with a [`WebSocketError::Protocol`] error.
/// - [`Sink::poll_close`] (e.g. `SinkExt::close`) is idempotent: the first call
///   emits a close frame with code 1000 and an empty reason unless a close frame
///   was already sent, then flushes it; subsequent calls only flush and return
///   `Ready(Ok(()))` without emitting another frame.
pub struct WebSocketSender {
    pub(crate) inner: Pin<Box<dyn AnyAsyncWebSocketSender>>,
}

/// The receiving half of an asynchronous WebSocket.
///
/// Implements [`Stream`] with `Item = WebSocketResult<WebSocketMessage>`.
/// Use `StreamExt` (e.g. via `futures-util`) to poll messages.
///
/// ## Close semantics
///
/// A peer-initiated close is delivered exactly once as
/// `Some(Ok(`[`WebSocketMessage::Close`]`))`. Every subsequent `poll_next` returns
/// `Poll::Ready(None)`: the stream is fused after the close frame.
///
/// A fatal transport failure is delivered once as `Some(Err(_))`, after which the
/// stream likewise terminates with `None`. Recoverable per-message failures
/// ([`WebSocketError::MessageTooLarge`], invalid UTF-8 payloads) are also delivered
/// as `Some(Err(_))` but do not necessarily terminate the stream; whether further
/// messages can be received afterwards is backend-defined, so applications should
/// treat the connection as unusable after any error.
pub struct WebSocketReceiver {
    pub(crate) inner: Pin<Box<dyn AnyAsyncWebSocketReceiver>>,
}

/// Backend implementation of an asynchronous WebSocket receiver.
///
/// Backend types implement [`Stream`] with `Item = WebSocketResult<WebSocketMessage>`
/// to provide incoming messages. This trait is automatically implemented for all
/// such streams.
///
/// Implementations must yield at most one [`WebSocketMessage::Close`] message;
/// after it (or after a fatal transport error), `poll_next` must return
/// `Poll::Ready(None)`. Per-message failures such as
/// [`WebSocketError::MessageTooLarge`] may be yielded as `Some(Err(_))` without
/// terminating the stream.
pub trait AsyncWebSocketReceiver:
    Stream<Item = WebSocketResult<WebSocketMessage>> + Send + 'static
{
}

impl<T> AsyncWebSocketReceiver for T where
    T: Stream<Item = WebSocketResult<WebSocketMessage>> + Send + 'static
{
}

/// Backend implementation of an asynchronous WebSocket sender.
///
/// Backend types implement [`Sink`] with `Item = WebSocketMessage` and
/// `Error = WebSocketError` to accept outgoing messages. This trait is
/// automatically implemented for all such sinks.
///
/// Implementations must emit at most one close frame per connection, however it
/// is initiated: `poll_close` must send a close frame with code 1000 and an
/// empty reason only if no close frame was previously sent via `start_send`,
/// then flush; repeated `poll_close` calls only flush. `start_send` after close
/// initiation (by either path) must fail with [`WebSocketError::Protocol`].
pub trait AsyncWebSocketSender:
    Sink<WebSocketMessage, Error = WebSocketError> + Send + 'static
{
}

impl<T> AsyncWebSocketSender for T where
    T: Sink<WebSocketMessage, Error = WebSocketError> + Send + 'static
{
}

impl WebSocketSender {
    /// Creates a sender from a backend implementation.
    pub fn from_impl<T: AsyncWebSocketSender>(sender: T) -> Self {
        Self {
            inner: Box::pin(sender),
        }
    }
}

impl WebSocketReceiver {
    /// Creates a receiver from a backend implementation.
    pub fn from_impl<T: AsyncWebSocketReceiver>(receiver: T) -> Self {
        Self {
            inner: Box::pin(receiver),
        }
    }
}

impl Sink<WebSocketMessage> for WebSocketSender {
    type Error = WebSocketError;

    fn poll_ready(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.get_mut().inner.as_mut().poll_ready(cx)
    }

    fn start_send(self: Pin<&mut Self>, item: WebSocketMessage) -> Result<(), Self::Error> {
        self.get_mut().inner.as_mut().start_send(item)
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.get_mut().inner.as_mut().poll_flush(cx)
    }

    fn poll_close(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.get_mut().inner.as_mut().poll_close(cx)
    }
}

impl Stream for WebSocketReceiver {
    type Item = WebSocketResult<WebSocketMessage>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        self.get_mut().inner.as_mut().poll_next(cx)
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
