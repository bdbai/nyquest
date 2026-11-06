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
    read_fragment, ws_error, ws_error_context, Received,
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

    ws.websocket
        .ConnectAsync(&uri)
        .into_nyquest_result()
        .map_err(ws_error)?
        .await
        .into_nyquest_result()
        .map_err(ws_error)?;

    let protocol = extract_negotiated_protocol(&ws.websocket)?;

    Ok(nyquest_interface::r#async::websocket::WebSocket::new(
        WebSocketInfo { protocol },
        nyquest_interface::r#async::websocket::WebSocketSender::from_impl(AsyncSender(ws.clone())),
        nyquest_interface::r#async::websocket::WebSocketReceiver::from_impl(AsyncReceiver {
            ws,
            rx,
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
    ws: Arc<AsyncSocket>,
    rx: Receiver<Received>,
    max_size: Option<usize>,
}

impl AsyncReceiver {
    async fn next_received(&mut self) -> WebSocketResult<Received> {
        let rx = &mut self.rx;
        let ws = &self.ws;
        poll_fn(move |cx| {
            let rx: &mut Receiver<Received> = rx;
            if let Ok(item) = rx.try_recv() {
                return Poll::Ready(Ok(item));
            }
            ws.waker.register(cx.waker());
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
