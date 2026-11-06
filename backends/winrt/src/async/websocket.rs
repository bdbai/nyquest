use std::borrow::Cow;
use std::future::poll_fn;
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::sync::Arc;
use std::task::Poll;

use futures_util::task::AtomicWaker;
use nyquest_interface::websocket::{
    WebSocketError, WebSocketInfo, WebSocketMessage, WebSocketResult,
};
use windows::Networking::Sockets::MessageWebSocket;

use crate::client::WinrtClient;
use crate::error::IntoNyquestResult;
use crate::websocket::{
    detach_writer, extract_negotiated_protocol, finalize_message, prepare_send_message,
    prepare_websocket, read_fragment, ws_error, ws_error_context, Received,
};

struct AsyncSocket {
    websocket: MessageWebSocket,
}

impl Drop for AsyncSocket {
    fn drop(&mut self) {
        let _ = self.websocket.Close();
    }
}

pub(super) async fn connect_async(
    client: WinrtClient,
    uri: Cow<'static, str>,
    protocols: Vec<Cow<'static, str>>,
) -> WebSocketResult<nyquest_interface::r#async::websocket::WebSocket> {
    let max_size = client
        .max_response_buffer_size
        .map(|size| usize::try_from(size).unwrap_or(usize::MAX));

    let (tx, rx) = mpsc::sync_channel(4);
    let waker = Arc::new(AtomicWaker::new());

    let event_tx = tx.clone();
    let event_waker = waker.clone();
    let close_tx = tx;
    let close_waker = waker.clone();

    let (websocket, uri_parsed) = prepare_websocket(
        &client,
        uri,
        protocols,
        move |item| {
            let _ = event_tx.send(item);
            event_waker.wake();
        },
        move |item| {
            let _ = close_tx.send(item);
            close_waker.wake();
        },
    )?;

    websocket
        .ConnectAsync(&uri_parsed)
        .into_nyquest_result()
        .map_err(ws_error)?
        .await
        .into_nyquest_result()
        .map_err(ws_error)?;

    let protocol = extract_negotiated_protocol(&websocket)?;
    let socket = Arc::new(AsyncSocket { websocket });

    Ok(nyquest_interface::r#async::websocket::WebSocket::new(
        WebSocketInfo { protocol },
        nyquest_interface::r#async::websocket::WebSocketSender::from_impl(AsyncSender(
            socket.clone(),
        )),
        nyquest_interface::r#async::websocket::WebSocketReceiver::from_impl(AsyncReceiver {
            _socket: socket,
            rx,
            waker,
            max_size,
        }),
    ))
}

struct AsyncSender(Arc<AsyncSocket>);

impl nyquest_interface::r#async::websocket::AsyncWebSocketSender for AsyncSender {
    async fn send(&mut self, data: WebSocketMessage) -> WebSocketResult<()> {
        let Some((writer, store_op)) = prepare_send_message(&self.0.websocket, data)? else {
            return Ok(());
        };
        store_op
            .await
            .into_nyquest_result()
            .map_err(|error| ws_error_context("store", error))?;
        detach_writer(writer)
    }

    async fn close(&mut self) -> WebSocketResult<()> {
        self.send(WebSocketMessage::Close {
            code: 1000,
            reason: String::new(),
        })
        .await
    }
}

struct AsyncReceiver {
    _socket: Arc<AsyncSocket>,
    rx: Receiver<Received>,
    waker: Arc<AtomicWaker>,
    max_size: Option<usize>,
}

impl AsyncReceiver {
    async fn next_received(&mut self) -> WebSocketResult<Received> {
        let rx = &mut self.rx;
        let waker = &self.waker;
        poll_fn(move |cx| {
            let rx: &mut Receiver<Received> = rx;
            if let Ok(item) = rx.try_recv() {
                return Poll::Ready(Ok(item));
            }
            waker.register(cx.waker());
            match rx.try_recv() {
                Ok(item) => Poll::Ready(Ok(item)),
                Err(TryRecvError::Empty) => Poll::Pending,
                Err(TryRecvError::Disconnected) => {
                    Poll::Ready(Err(WebSocketError::Io(std::io::Error::new(
                        std::io::ErrorKind::UnexpectedEof,
                        "WebSocket receive channel closed",
                    ))))
                }
            }
        })
        .await
    }
}

impl nyquest_interface::r#async::websocket::AsyncWebSocketReceiver for AsyncReceiver {
    async fn receive(&mut self) -> WebSocketResult<WebSocketMessage> {
        let max_size = self.max_size;
        let mut bytes = Vec::new();
        loop {
            match self.next_received().await? {
                Received::Closed { code, reason } => {
                    return Ok(WebSocketMessage::Close { code, reason });
                }
                Received::Error(err) => {
                    return Err(err);
                }
                Received::Message {
                    message_type,
                    reader,
                    complete,
                } => {
                    read_fragment(&reader, &mut bytes, max_size)?;
                    if complete {
                        return finalize_message(message_type, bytes);
                    }
                }
            }
        }
    }
}
