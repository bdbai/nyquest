use std::borrow::Cow;
use std::sync::{Arc, OnceLock};

use futures::{SinkExt, StreamExt};
use nyquest_interface::websocket::{WebSocketError, WebSocketMessage, WebSocketResult};
use tokio::runtime::Runtime;

use super::execute_with_runtime;
use crate::client::ReqwestClient;
use crate::websocket::{
    connect_parts, from_tungstenite_message, into_tungstenite_message, map_error, Sink, Source,
};

struct BlockingSender {
    sink: Sink,
    runtime: Arc<OnceLock<Runtime>>,
}

struct BlockingReceiver {
    source: Source,
    runtime: Arc<OnceLock<Runtime>>,
}

pub(super) fn connect_blocking(
    client: &ReqwestClient,
    uri: Cow<'static, str>,
    protocol: Option<Cow<'static, str>>,
) -> WebSocketResult<nyquest_interface::blocking::websocket::WebSocket> {
    let base_url = client.base_url.clone();
    let max_message_size = client.max_response_buffer_size;
    let runtime = client.managed_runtime.clone();
    let (sink, source) = execute_with_runtime(&runtime, || async move {
        connect_parts(base_url.as_ref(), uri, protocol, max_message_size).await
    })?;
    let sender =
        nyquest_interface::blocking::websocket::WebSocketSender::from_impl(BlockingSender {
            sink,
            runtime: runtime.clone(),
        });
    let receiver =
        nyquest_interface::blocking::websocket::WebSocketReceiver::from_impl(BlockingReceiver {
            source,
            runtime,
        });
    Ok(nyquest_interface::blocking::websocket::WebSocket::new(
        sender, receiver,
    ))
}

impl nyquest_interface::blocking::websocket::BlockingWebSocketSender for BlockingSender {
    fn send(&mut self, data: WebSocketMessage) -> WebSocketResult<()> {
        let sink = &mut self.sink;
        let message = into_tungstenite_message(data);
        execute_with_runtime(&self.runtime, || async move {
            sink.send(message).await.map_err(map_error)
        })
    }

    fn close(&mut self) -> WebSocketResult<()> {
        let sink = &mut self.sink;
        execute_with_runtime(&self.runtime, || async move {
            sink.close().await.map_err(map_error)
        })
    }
}

impl nyquest_interface::blocking::websocket::BlockingWebSocketReceiver for BlockingReceiver {
    fn receive(&mut self) -> WebSocketResult<WebSocketMessage> {
        let source = &mut self.source;
        execute_with_runtime(&self.runtime, || async move {
            loop {
                let message = source
                    .next()
                    .await
                    .ok_or_else(|| WebSocketError::Protocol("WebSocket stream ended".into()))?
                    .map_err(map_error)?;
                if let Some(msg) = from_tungstenite_message(message) {
                    return Ok(msg);
                }
            }
        })
    }
}
