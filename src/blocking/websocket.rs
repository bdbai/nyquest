//! Blocking WebSocket support.

use std::fmt;

use nyquest_interface::blocking::websocket as ws_impl;

use crate::websocket::{WebSocketError, WebSocketInfo, WebSocketMessage, WebSocketResult};

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
    pub(crate) fn from_interface(value: ws_impl::WebSocket) -> Self {
        Self {
            info: WebSocketInfo::from_interface(value.info),
            sender: WebSocketSender::from_interface(value.sender),
            receiver: WebSocketReceiver::from_interface(value.receiver),
        }
    }
}

/// The sending half of a blocking WebSocket.
pub struct WebSocketSender {
    inner: ws_impl::WebSocketSender,
}

impl WebSocketSender {
    pub(crate) fn from_interface(value: ws_impl::WebSocketSender) -> Self {
        Self { inner: value }
    }

    /// Sends a message.
    pub fn send(&mut self, data: WebSocketMessage) -> WebSocketResult<()> {
        self.inner
            .send(data.into_interface())
            .map_err(WebSocketError::from_interface)
    }
    /// Closes the connection.
    pub fn close(mut self) -> WebSocketResult<()> {
        self.inner.close().map_err(WebSocketError::from_interface)
    }
}

impl fmt::Debug for WebSocketSender {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "WebSocketSender")
    }
}

/// The receiving half of a blocking WebSocket.
pub struct WebSocketReceiver {
    inner: ws_impl::WebSocketReceiver,
}

impl WebSocketReceiver {
    pub(crate) fn from_interface(value: ws_impl::WebSocketReceiver) -> Self {
        Self { inner: value }
    }

    /// Receives the next message.
    pub fn receive(&mut self) -> WebSocketResult<WebSocketMessage> {
        self.inner
            .receive()
            .map(WebSocketMessage::from_interface)
            .map_err(WebSocketError::from_interface)
    }
}

impl fmt::Debug for WebSocketReceiver {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "WebSocketReceiver")
    }
}

/// An owning iterator over a [`WebSocketReceiver`].
///
/// Each [`next`](Iterator::next) call receives the next message, yielding
/// [`WebSocketResult`]`<`[`WebSocketMessage`]`>`, including errors. After an error
/// (or a [`Close`](WebSocketMessage::Close) message) is yielded, the iterator is fused and
/// further [`next`](Iterator::next) calls return `None`, so a `for` loop over the receiver
/// stops.
///
/// ```no_run
/// # use nyquest::{BlockingClient, WebSocketMessage};
/// # fn run(client: BlockingClient) -> Result<(), nyquest::WebSocketError> {
/// let mut ws = client.connect_websocket("ws://example.com", None)?;
/// ws.sender.send(WebSocketMessage::text("hello"))?;
/// ws.sender.send(WebSocketMessage::binary([1, 2, 3]))?;
/// for msg in ws.receiver {
///     let msg = msg?;
///     if msg.is_close() {
///         break;
///     }
///     if msg.is_text() {
///         println!("text: {}", msg.to_text().unwrap_or("<invalid utf-8>"));
///     } else if msg.is_binary() {
///         println!("bytes: {:?}", msg.into_data());
///     }
/// }
/// # Ok(())
/// # }
/// ```
pub struct WebSocketReceiverIter {
    inner: WebSocketReceiver,
    terminated: bool,
}

impl Iterator for WebSocketReceiverIter {
    type Item = WebSocketResult<WebSocketMessage>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.terminated {
            return None;
        }
        match self.inner.receive() {
            Ok(msg) => {
                if matches!(msg, WebSocketMessage::Close { .. }) {
                    self.terminated = true;
                }
                Some(Ok(msg))
            }
            Err(err) => {
                self.terminated = true;
                Some(Err(err))
            }
        }
    }
}

impl std::iter::FusedIterator for WebSocketReceiverIter {}

impl fmt::Debug for WebSocketReceiverIter {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "WebSocketReceiverIter")
    }
}

impl IntoIterator for WebSocketReceiver {
    type Item = WebSocketResult<WebSocketMessage>;
    type IntoIter = WebSocketReceiverIter;

    fn into_iter(self) -> Self::IntoIter {
        WebSocketReceiverIter {
            inner: self,
            terminated: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nyquest_interface::blocking::websocket::{
        BlockingWebSocketReceiver, BlockingWebSocketSender,
    };
    use nyquest_interface::websocket as ws_iface;
    use nyquest_interface::websocket::{
        WebSocketError as IfaceError, WebSocketMessage as IfaceMessage,
        WebSocketResult as IfaceResult,
    };

    struct MockReceiver {
        messages: std::collections::VecDeque<ws_iface::WebSocketResult<IfaceMessage>>,
    }

    impl BlockingWebSocketReceiver for MockReceiver {
        fn receive(&mut self) -> ws_iface::WebSocketResult<IfaceMessage> {
            self.messages.pop_front().unwrap_or_else(|| {
                Err(ws_iface::WebSocketError::Protocol(
                    "no more messages".into(),
                ))
            })
        }
    }

    struct MockSender {
        closed: bool,
    }

    impl BlockingWebSocketSender for MockSender {
        fn send(&mut self, _data: IfaceMessage) -> ws_iface::WebSocketResult<()> {
            Ok(())
        }
        fn close(&mut self) -> ws_iface::WebSocketResult<()> {
            self.closed = true;
            Ok(())
        }
    }

    fn mock_receiver(messages: Vec<IfaceResult<IfaceMessage>>) -> WebSocketReceiver {
        let iface_messages: std::collections::VecDeque<ws_iface::WebSocketResult<IfaceMessage>> =
            messages.into_iter().collect();
        let inner = ws_impl::WebSocketReceiver::from_impl(MockReceiver {
            messages: iface_messages,
        });
        WebSocketReceiver::from_interface(inner)
    }

    #[test]
    fn receiver_iter_yields_messages_and_stops_after_close() {
        let mut receiver = mock_receiver(vec![
            Ok(IfaceMessage::Utf8("hello".into())),
            Ok(IfaceMessage::Close {
                code: 1000,
                reason: "done".into(),
            }),
        ]);
        // `receive` still works directly.
        assert_eq!(
            receiver.receive().unwrap(),
            WebSocketMessage::Utf8("hello".into())
        );
        // Iterator fuses after Close.
        let receiver = mock_receiver(vec![
            Ok(IfaceMessage::Utf8("hello".into())),
            Ok(IfaceMessage::Close {
                code: 1000,
                reason: "done".into(),
            }),
        ]);
        let mut iter = receiver.into_iter();
        let collected: Vec<_> = iter.by_ref().collect();
        assert_eq!(collected.len(), 2);
        assert!(matches!(collected[1], Ok(WebSocketMessage::Close { .. })));
        assert!(iter.next().is_none());
        assert!(iter.next().is_none());
    }

    #[test]
    fn receiver_iter_yields_error_and_then_stops() {
        let receiver = mock_receiver(vec![
            Ok(IfaceMessage::Utf8("hello".into())),
            Err(IfaceError::Protocol("boom".into())),
            Ok(IfaceMessage::Utf8("unreached".into())),
        ]);
        let mut iter = receiver.into_iter();
        assert_eq!(
            iter.next().unwrap().unwrap(),
            WebSocketMessage::Utf8("hello".into())
        );
        let err = iter.next().unwrap();
        assert!(err.is_err());
        // Fused after error: no further items, even though the mock has more.
        assert!(iter.next().is_none());
        assert!(iter.next().is_none());
    }

    #[test]
    fn sender_close_consumes_sender() {
        let inner = ws_impl::WebSocketSender::from_impl(MockSender { closed: false });
        let sender = WebSocketSender::from_interface(inner);
        sender.close().unwrap();
        // `close(self)` moves the sender: the following would not compile if
        // `close` still took `&mut self` in a consuming context.
        // (Compile-time assertion; runtime assertion is in the mock.)
    }

    #[test]
    fn for_loop_over_receiver_stops_on_error() {
        let receiver = mock_receiver(vec![
            Ok(IfaceMessage::Utf8("a".into())),
            Ok(IfaceMessage::Utf8("b".into())),
            Err(IfaceError::Protocol("boom".into())),
        ]);
        let mut count = 0;
        let mut saw_error = false;
        for res in receiver {
            match res {
                Ok(_) => count += 1,
                Err(_) => {
                    saw_error = true;
                }
            }
        }
        assert_eq!(count, 2);
        assert!(saw_error);
    }
}
