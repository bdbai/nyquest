use std::borrow::Cow;
use std::sync::mpsc::{self, Receiver};
use std::sync::Arc;

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

struct BlockingSocket {
    websocket: MessageWebSocket,
}

impl Drop for BlockingSocket {
    fn drop(&mut self) {
        let _ = self.websocket.Close();
    }
}

pub(super) fn connect_blocking(
    client: &WinrtClient,
    uri: Cow<'static, str>,
    protocols: Vec<Cow<'static, str>>,
) -> WebSocketResult<nyquest_interface::blocking::websocket::WebSocket> {
    let uri = client.validate_ws_uri(&uri)?;
    let websocket = MessageWebSocket::new()
        .into_nyquest_result()
        .map_err(ws_error)?;

    let (tx, rx) = mpsc::channel();
    let on_event = move |item| {
        let _ = tx.send(item);
    };
    client.prepare_ws(&websocket, protocols, on_event.clone(), on_event)?;

    websocket
        .ConnectAsync(&uri)
        .into_nyquest_result()
        .map_err(ws_error)?
        .join()
        .into_nyquest_result()
        .map_err(ws_error)?;

    let protocol = extract_negotiated_protocol(&websocket)?;
    let socket = Arc::new(BlockingSocket { websocket });
    let max_size = client
        .max_response_buffer_size
        .and_then(|size| usize::try_from(size).ok());

    Ok(nyquest_interface::blocking::websocket::WebSocket::new(
        WebSocketInfo { protocol },
        nyquest_interface::blocking::websocket::WebSocketSender::from_impl(Sender(socket.clone())),
        nyquest_interface::blocking::websocket::WebSocketReceiver::from_impl(ReceiverImpl {
            _socket: socket,
            rx,
            max_size,
        }),
    ))
}

struct Sender(Arc<BlockingSocket>);

impl nyquest_interface::blocking::websocket::BlockingWebSocketSender for Sender {
    fn send(&mut self, data: WebSocketMessage) -> WebSocketResult<()> {
        let Some((writer, store_op)) = prepare_send_message(&self.0.websocket, data)? else {
            return Ok(());
        };
        store_op
            .join()
            .into_nyquest_result()
            .map_err(|error| ws_error_context("store", error))?;
        detach_writer(writer)
    }

    fn close(&mut self) -> WebSocketResult<()> {
        self.send(WebSocketMessage::Close {
            code: 1000,
            reason: String::new(),
        })
    }
}

struct ReceiverImpl {
    _socket: Arc<BlockingSocket>,
    rx: Receiver<Received>,
    max_size: Option<usize>,
}

impl nyquest_interface::blocking::websocket::BlockingWebSocketReceiver for ReceiverImpl {
    fn receive(&mut self) -> WebSocketResult<WebSocketMessage> {
        let mut bytes = Vec::new();
        loop {
            let item = self.rx.recv().map_err(|_| {
                WebSocketError::Io(std::io::Error::new(
                    std::io::ErrorKind::UnexpectedEof,
                    "WebSocket receive channel closed",
                ))
            })?;
            match item {
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
                    read_fragment(&reader, &mut bytes, self.max_size)?;
                    if complete {
                        return finalize_message(message_type, bytes);
                    }
                }
            }
        }
    }
}
