//! Asynchronous WebSocket support.

use std::fmt;
use std::pin::Pin;
use std::task::{Context, Poll};

use nyquest_interface::r#async::websocket as ws_impl;
use nyquest_interface::r#async::{futures_core, futures_sink};

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
    pub(crate) fn from_interface(value: ws_impl::WebSocket) -> Self {
        Self {
            info: WebSocketInfo::from_interface(value.info),
            sender: WebSocketSender::from_interface(value.sender),
            receiver: WebSocketReceiver::from_interface(value.receiver),
        }
    }
}

/// The sending half of an asynchronous WebSocket.
///
/// Implements [`Sink`](futures_sink::Sink) with `Item = WebSocketMessage` and
/// `Error = WebSocketError`. Use `SinkExt` (e.g. via `futures-util`) to `send` messages and
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
/// - [`Sink::poll_close`](futures_sink::Sink::poll_close) (e.g. `SinkExt::close`) is idempotent:
///   the first call emits a close frame with code 1000 and an empty reason unless a close frame
///   was already sent, then flushes it; subsequent calls only flush and return
///   `Ready(Ok(()))` without emitting another frame.
pub struct WebSocketSender {
    inner: ws_impl::WebSocketSender,
}

impl WebSocketSender {
    pub(crate) fn from_interface(value: ws_impl::WebSocketSender) -> Self {
        Self { inner: value }
    }
}

impl futures_sink::Sink<WebSocketMessage> for WebSocketSender {
    type Error = WebSocketError;

    fn poll_ready(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Pin::new(&mut self.get_mut().inner)
            .poll_ready(cx)
            .map_err(WebSocketError::from_interface)
    }

    fn start_send(self: Pin<&mut Self>, item: WebSocketMessage) -> Result<(), Self::Error> {
        Pin::new(&mut self.get_mut().inner)
            .start_send(item.into_interface())
            .map_err(WebSocketError::from_interface)
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Pin::new(&mut self.get_mut().inner)
            .poll_flush(cx)
            .map_err(WebSocketError::from_interface)
    }

    fn poll_close(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Pin::new(&mut self.get_mut().inner)
            .poll_close(cx)
            .map_err(WebSocketError::from_interface)
    }
}

impl fmt::Debug for WebSocketSender {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "WebSocketSender")
    }
}

/// The receiving half of an asynchronous WebSocket.
///
/// Implements [`Stream`](futures_core::Stream) with `Item = WebSocketResult<WebSocketMessage>`.
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
    inner: ws_impl::WebSocketReceiver,
}

impl WebSocketReceiver {
    pub(crate) fn from_interface(value: ws_impl::WebSocketReceiver) -> Self {
        Self { inner: value }
    }
}

impl futures_core::Stream for WebSocketReceiver {
    type Item = WebSocketResult<WebSocketMessage>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        Pin::new(&mut self.get_mut().inner)
            .poll_next(cx)
            .map(|opt| {
                opt.map(|res| {
                    res.map(WebSocketMessage::from_interface)
                        .map_err(WebSocketError::from_interface)
                })
            })
    }
}

impl fmt::Debug for WebSocketReceiver {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "WebSocketReceiver")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures_core::Stream;
    use futures_sink::Sink;
    use nyquest_interface::websocket as ws_iface;
    use nyquest_interface::websocket::WebSocketMessage as IfaceMessage;

    struct MockStream {
        messages: std::collections::VecDeque<ws_iface::WebSocketResult<IfaceMessage>>,
    }

    impl Stream for MockStream {
        type Item = ws_iface::WebSocketResult<IfaceMessage>;

        fn poll_next(mut self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
            Poll::Ready(self.messages.pop_front())
        }
    }

    struct MockSink {
        sent: Vec<IfaceMessage>,
        closed: bool,
    }

    impl Sink<IfaceMessage> for MockSink {
        type Error = ws_iface::WebSocketError;

        fn poll_ready(
            self: Pin<&mut Self>,
            _cx: &mut Context<'_>,
        ) -> Poll<Result<(), Self::Error>> {
            Poll::Ready(Ok(()))
        }

        fn start_send(mut self: Pin<&mut Self>, item: IfaceMessage) -> Result<(), Self::Error> {
            self.sent.push(item);
            Ok(())
        }

        fn poll_flush(
            self: Pin<&mut Self>,
            _cx: &mut Context<'_>,
        ) -> Poll<Result<(), Self::Error>> {
            Poll::Ready(Ok(()))
        }

        fn poll_close(
            mut self: Pin<&mut Self>,
            _cx: &mut Context<'_>,
        ) -> Poll<Result<(), Self::Error>> {
            self.closed = true;
            Poll::Ready(Ok(()))
        }
    }

    #[test]
    fn async_wrappers_forward_stream_and_sink() {
        let inner_rx = ws_impl::WebSocketReceiver::from_impl(MockStream {
            messages: [Ok(IfaceMessage::Utf8("hi".into()))].into(),
        });
        let mut receiver = WebSocketReceiver::from_interface(inner_rx);
        let next = futures::executor::block_on(futures::StreamExt::next(&mut receiver));
        assert_eq!(next.unwrap().unwrap(), WebSocketMessage::Utf8("hi".into()));

        let inner_tx = ws_impl::WebSocketSender::from_impl(MockSink {
            sent: Vec::new(),
            closed: false,
        });
        let mut sender = WebSocketSender::from_interface(inner_tx);
        futures::executor::block_on(futures::SinkExt::send(
            &mut sender,
            WebSocketMessage::Utf8("hello".into()),
        ))
        .unwrap();
        futures::executor::block_on(futures::SinkExt::close(&mut sender)).unwrap();
    }
}
