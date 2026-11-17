use std::borrow::Cow;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

use futures::{Sink, Stream};
use nyquest_interface::websocket::{WebSocketError, WebSocketMessage, WebSocketResult};
use pin_project_lite::pin_project;

use super::execute_with_runtime_async;
use crate::client::ReqwestClient;
use crate::runtime::ManagedRuntime;
use crate::websocket::{
    connect_parts, from_tungstenite_message, into_tungstenite_message, map_error,
    Sink as InnerSink, Source as InnerSource,
};

pin_project! {
    struct AsyncSender {
        #[pin]
        sink: InnerSink,
        runtime: Arc<ManagedRuntime>,
        closing: bool,
        close_sent: bool,
    }
}

pin_project! {
    struct AsyncReceiver {
        #[pin]
        source: InnerSource,
        runtime: Arc<ManagedRuntime>,
        terminated: bool,
    }
}

pub(super) async fn connect_async(
    client: &ReqwestClient,
    uri: Cow<'static, str>,
    protocols: Vec<Cow<'static, str>>,
) -> WebSocketResult<nyquest_interface::r#async::websocket::WebSocket> {
    let http_client = client.client.clone();
    let base_url = client.base_url.clone();
    let max_message_size = client.max_response_buffer_size;
    let runtime = client.managed_runtime.clone();
    let res = execute_with_runtime_async(&runtime, move || async move {
        connect_parts(
            http_client,
            base_url.as_ref(),
            uri,
            protocols,
            max_message_size,
        )
        .await
    })
    .await;
    let (info, sink, source) = res?;
    Ok(nyquest_interface::r#async::websocket::WebSocket::new(
        info,
        nyquest_interface::r#async::websocket::WebSocketSender::from_impl(AsyncSender {
            sink,
            runtime: runtime.clone(),
            closing: false,
            close_sent: false,
        }),
        nyquest_interface::r#async::websocket::WebSocketReceiver::from_impl(AsyncReceiver {
            source,
            runtime,
            terminated: false,
        }),
    ))
}

fn is_close(item: &WebSocketMessage) -> bool {
    matches!(item, WebSocketMessage::Close { .. })
}

fn closed_error() -> WebSocketError {
    WebSocketError::Protocol("WebSocket sender is closed".into())
}

impl Sink<WebSocketMessage> for AsyncSender {
    type Error = nyquest_interface::websocket::WebSocketError;

    fn poll_ready(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        let mut this = self.project();
        if *this.closing || *this.close_sent {
            return Poll::Ready(Err(closed_error()));
        }
        let _enter = this.runtime.get_handle_for_async().enter();
        this.sink.as_mut().poll_ready(cx).map_err(map_error)
    }

    fn start_send(self: Pin<&mut Self>, item: WebSocketMessage) -> Result<(), Self::Error> {
        let mut this = self.project();
        if *this.closing || *this.close_sent {
            return Err(closed_error());
        }
        let close = is_close(&item);
        let _enter = this.runtime.get_handle_for_async().enter();
        this.sink
            .as_mut()
            .start_send(into_tungstenite_message(item))
            .map_err(map_error)?;
        if close {
            *this.close_sent = true;
        }
        Ok(())
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        let mut this = self.project();
        let _enter = this.runtime.get_handle_for_async().enter();
        this.sink.as_mut().poll_flush(cx).map_err(map_error)
    }

    fn poll_close(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        let mut this = self.project();
        *this.closing = true;
        // A close frame sent via `start_send` already initiates the handshake;
        // only flush it instead of emitting a second one.
        if *this.close_sent {
            let _enter = this.runtime.get_handle_for_async().enter();
            return this.sink.as_mut().poll_flush(cx).map_err(map_error);
        }
        let _enter = this.runtime.get_handle_for_async().enter();
        this.sink.as_mut().poll_close(cx).map_err(map_error)
    }
}

impl Stream for AsyncReceiver {
    type Item = WebSocketResult<WebSocketMessage>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let mut this = self.project();
        if *this.terminated {
            return Poll::Ready(None);
        }
        loop {
            let next = {
                let _enter = this.runtime.get_handle_for_async().enter();
                this.source.as_mut().poll_next(cx)
            };
            match next {
                Poll::Ready(None) => return Poll::Ready(None),
                Poll::Ready(Some(Err(e))) => {
                    *this.terminated = true;
                    return Poll::Ready(Some(Err(map_error(e))));
                }
                Poll::Ready(Some(Ok(message))) => {
                    if let Some(msg) = from_tungstenite_message(message) {
                        if matches!(msg, WebSocketMessage::Close { .. }) {
                            *this.terminated = true;
                        }
                        return Poll::Ready(Some(Ok(msg)));
                    }
                    // Ping/Pong/Frame: poll again within the same task.
                    // If the underlying stream is now Pending, the waker is
                    // already registered; loop to re-poll.
                    // To avoid busy-loop when the underlying source keeps
                    // yielding skippable messages, re-poll directly.
                    continue;
                }
                Poll::Pending => return Poll::Pending,
            }
        }
    }
}
