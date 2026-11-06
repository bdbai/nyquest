use std::borrow::Cow;
use std::future::{poll_fn, Future};
use std::pin::pin;

use futures::{SinkExt, StreamExt};
use nyquest_interface::websocket::{WebSocketError, WebSocketMessage, WebSocketResult};
use tokio::runtime::Handle;

use super::execute_with_runtime_async;
use crate::client::ReqwestClient;
use crate::websocket::{
    connect_parts, from_tungstenite_message, into_tungstenite_message, map_error, Sink, Source,
};

struct AsyncSender {
    sink: Sink,
    handle: Handle,
}

struct AsyncReceiver {
    source: Source,
    handle: Handle,
}

pub(super) async fn connect_async(
    client: &ReqwestClient,
    uri: Cow<'static, str>,
    protocols: Vec<Cow<'static, str>>,
) -> WebSocketResult<nyquest_interface::r#async::websocket::WebSocket> {
    let base_url = client.base_url.clone();
    let max_message_size = client.max_response_buffer_size;
    let (res, handle) = execute_with_runtime_async(&client.managed_runtime, move || async move {
        connect_parts(base_url.as_ref(), uri, protocols, max_message_size).await
    })
    .await;
    let (info, sink, source) = res?;
    Ok(nyquest_interface::r#async::websocket::WebSocket::new(
        info,
        nyquest_interface::r#async::websocket::WebSocketSender::from_impl(AsyncSender {
            sink,
            handle: handle.clone(),
        }),
        nyquest_interface::r#async::websocket::WebSocketReceiver::from_impl(AsyncReceiver {
            source,
            handle,
        }),
    ))
}

impl nyquest_interface::r#async::websocket::AsyncWebSocketSender for AsyncSender {
    async fn send(&mut self, data: WebSocketMessage) -> WebSocketResult<()> {
        let message = into_tungstenite_message(data);
        let handle = &self.handle;
        let mut fut = pin!(self.sink.send(message));
        poll_fn(|cx| {
            let _enter = handle.enter();
            fut.as_mut().poll(cx)
        })
        .await
        .map_err(map_error)
    }

    async fn close(&mut self) -> WebSocketResult<()> {
        let handle = &self.handle;
        let mut fut = pin!(self.sink.close());
        poll_fn(|cx| {
            let _enter = handle.enter();
            fut.as_mut().poll(cx)
        })
        .await
        .map_err(map_error)
    }
}

impl nyquest_interface::r#async::websocket::AsyncWebSocketReceiver for AsyncReceiver {
    async fn receive(&mut self) -> WebSocketResult<WebSocketMessage> {
        let handle = &self.handle;
        loop {
            let next = {
                let mut fut = pin!(self.source.next());
                poll_fn(|cx| {
                    let _enter = handle.enter();
                    fut.as_mut().poll(cx)
                })
                .await
            };
            let message = next
                .ok_or_else(|| WebSocketError::Protocol("WebSocket stream ended".into()))?
                .map_err(map_error)?;
            if let Some(msg) = from_tungstenite_message(message) {
                return Ok(msg);
            }
        }
    }
}
