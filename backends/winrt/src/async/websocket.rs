use std::borrow::Cow;
use std::future::{Future as _, IntoFuture};
use std::pin::Pin;
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::sync::Arc;
use std::task::{Context, Poll};

use futures_core::Stream;
use futures_sink::Sink;
use futures_util::task::AtomicWaker;
use nyquest_interface::websocket::{
    WebSocketError, WebSocketInfo, WebSocketMessage, WebSocketResult,
};
use windows::Networking::Sockets::MessageWebSocket;
use windows::Storage::Streams::{DataWriter, DataWriterStoreOperation};

use crate::client::WinrtClient;
use crate::error::IntoNyquestResult;
use crate::r#async::timer_ext::AsyncTimeoutExt;
use crate::timer::Timer;
use crate::websocket::{
    detach_writer, extract_negotiated_protocol, finalize_message, prepare_send_message,
    read_fragment, ws_error, ws_error_context, DetachableWriter, Received,
};

struct AsyncSocket {
    websocket: MessageWebSocket,
    waker: AtomicWaker,
}

impl Drop for AsyncSocket {
    fn drop(&mut self) {
        let _ = self.websocket.Close();
    }
}

pub(super) async fn connect_async(
    client: &WinrtClient,
    uri: Cow<'static, str>,
    protocols: Vec<Cow<'static, str>>,
) -> WebSocketResult<nyquest_interface::r#async::websocket::WebSocket> {
    let uri = client.validate_ws_uri(&uri)?;
    let ws = MessageWebSocket::new()
        .into_nyquest_result()
        .map_err(ws_error)?;

    let max_size = client
        .max_response_buffer_size
        .map(|size| usize::try_from(size).unwrap_or(usize::MAX));

    let (tx, rx) = mpsc::sync_channel(4);
    let ws = Arc::new(AsyncSocket {
        websocket: ws,
        waker: AtomicWaker::new(),
    });
    let on_event = {
        let socket = Arc::downgrade(&ws);
        move |item| {
            let Some(ws) = socket.upgrade() else {
                return;
            };
            let _ = tx.send(item);
            ws.waker.wake();
        }
    };

    client.prepare_ws(&ws.websocket, protocols, on_event.clone(), on_event)?;

    let mut timer = Timer::new(client.request_timeout);
    ws.websocket
        .ConnectAsync(&uri)
        .into_nyquest_result()
        .map_err(ws_error)?
        .timeout_by(&mut timer)
        .await
        .map_err(ws_error)?;

    let protocol = extract_negotiated_protocol(&ws.websocket)?;

    Ok(nyquest_interface::r#async::websocket::WebSocket::new(
        WebSocketInfo { protocol },
        nyquest_interface::r#async::websocket::WebSocketSender::from_impl(AsyncSender {
            socket: ws.clone(),
            pending: None,
            closing: false,
            close_sent: false,
        }),
        nyquest_interface::r#async::websocket::WebSocketReceiver::from_impl(AsyncReceiver {
            ws,
            rx,
            max_size,
            bytes: Vec::new(),
            terminated: false,
        }),
    ))
}

/// A single in-flight `DataWriter::StoreAsync` operation.
///
/// The future is boxed so that `AsyncSender` only contains `Unpin` fields and
/// `Sink` can be implemented via `Pin::get_mut` without unsafe code.
struct PendingSend {
    writer: DetachableWriter,
    store_fut: Pin<Box<StoreFut>>,
}

type StoreFut = <DataWriterStoreOperation as IntoFuture>::IntoFuture;

struct AsyncSender {
    socket: Arc<AsyncSocket>,
    pending: Option<PendingSend>,
    closing: bool,
    close_sent: bool,
}

fn closed_error() -> WebSocketError {
    WebSocketError::Protocol("WebSocket sender is closed".into())
}

impl AsyncSender {
    fn poll_pending(&mut self, cx: &mut Context<'_>) -> Poll<WebSocketResult<()>> {
        let Some(pending) = self.pending.as_mut() else {
            return Poll::Ready(Ok(()));
        };
        match pending.store_fut.as_mut().poll(cx) {
            Poll::Ready(Ok(_)) => {
                let pending = self.pending.take().expect("pending checked");
                match detach_writer(pending.writer) {
                    Ok(()) => Poll::Ready(Ok(())),
                    Err(e) => Poll::Ready(Err(e)),
                }
            }
            Poll::Ready(Err(e)) => {
                // Drop the writer (its Drop detaches) and report the error.
                self.pending.take();
                Poll::Ready(Err(ws_error_context("store", e)))
            }
            Poll::Pending => Poll::Pending,
        }
    }
}

impl Sink<WebSocketMessage> for AsyncSender {
    type Error = WebSocketError;

    fn poll_ready(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        // All fields are `Unpin`, so pinning is trivial.
        let this = self.get_mut();
        if this.closing || this.close_sent {
            return Poll::Ready(Err(closed_error()));
        }
        if this.pending.is_some() {
            // Drive the in-flight send so `poll_ready` does not deadlock
            // when the caller only polls readiness.
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
        let Some((writer, store_op)) = prepare_send_message(&this.socket.websocket, item)? else {
            // Close frame was sent synchronously via CloseWithStatus.
            this.close_sent = true;
            return Ok(());
        };
        this.pending = Some(PendingSend {
            writer,
            store_fut: Box::pin(store_op.into_future()),
        });
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
        // Send the close frame synchronously. `prepare_send_message` with Close
        // calls CloseWithStatus and returns None.
        let close_result = prepare_send_message(
            &this.socket.websocket,
            WebSocketMessage::Close {
                code: 1000,
                reason: String::new(),
            },
        );
        match close_result {
            Ok(None) => {
                this.close_sent = true;
                Poll::Ready(Ok(()))
            }
            Ok(Some(_)) => unreachable!("close should not produce a store operation"),
            Err(e) => Poll::Ready(Err(e)),
        }
    }
}

struct AsyncReceiver {
    ws: Arc<AsyncSocket>,
    rx: Receiver<Received>,
    max_size: Option<usize>,
    bytes: Vec<u8>,
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
        loop {
            let item = match this.rx.try_recv() {
                Ok(item) => item,
                Err(TryRecvError::Empty) => {
                    this.ws.waker.register(cx.waker());
                    match this.rx.try_recv() {
                        Ok(item) => item,
                        Err(TryRecvError::Empty) => return Poll::Pending,
                        Err(TryRecvError::Disconnected) => {
                            // The event source is gone; report termination once.
                            this.terminated = true;
                            return Poll::Ready(Some(Err(WebSocketError::Io(
                                std::io::Error::new(
                                    std::io::ErrorKind::UnexpectedEof,
                                    "WebSocket receive channel closed",
                                ),
                            ))));
                        }
                    }
                }
                Err(TryRecvError::Disconnected) => {
                    this.terminated = true;
                    return Poll::Ready(Some(Err(WebSocketError::Io(std::io::Error::new(
                        std::io::ErrorKind::UnexpectedEof,
                        "WebSocket receive channel closed",
                    )))));
                }
            };
            match item {
                Received::Closed { code, reason } => {
                    this.bytes.clear();
                    this.terminated = true;
                    return Poll::Ready(Some(Ok(WebSocketMessage::Close { code, reason })));
                }
                Received::Error(err) => {
                    this.bytes.clear();
                    return Poll::Ready(Some(Err(err)));
                }
                Received::Message {
                    message_type,
                    reader,
                    complete,
                } => {
                    if let Err(err) = read_fragment(&reader, &mut this.bytes, this.max_size) {
                        this.bytes.clear();
                        return Poll::Ready(Some(Err(err)));
                    }
                    if complete {
                        let bytes = std::mem::take(&mut this.bytes);
                        match finalize_message(message_type, bytes) {
                            Ok(msg) => return Poll::Ready(Some(Ok(msg))),
                            Err(e) => return Poll::Ready(Some(Err(e))),
                        }
                    }
                    // Fragment accumulated; try next queued event without returning Pending.
                    continue;
                }
            }
        }
    }
}

// Silence unused import warning for DataWriter (used indirectly via DetachableWriter).
#[allow(dead_code)]
fn _assert_datawriter(_: &DataWriter) {}
