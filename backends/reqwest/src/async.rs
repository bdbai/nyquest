use std::borrow::Cow;
use std::future::{poll_fn, Future};
use std::pin::{pin, Pin};
use std::sync::Arc;

use futures::future::{select, Either};
use nyquest_interface::client::ClientOptions;
use nyquest_interface::r#async::{AsyncClient, AsyncResponse, Request};
use nyquest_interface::Result as NyquestResult;

use crate::client::ReqwestClient;
use crate::customizer::Customizer;
use crate::error::{ReqwestBackendError, Result};
use crate::response::ReqwestResponse;
use crate::runtime::ManagedRuntime;

#[cfg(feature = "async-stream")]
mod stream;
#[cfg(feature = "ws")]
mod websocket;

#[derive(Clone)]
pub struct ReqwestAsyncClient {
    inner: ReqwestClient,
}

impl ReqwestAsyncClient {
    pub(crate) fn new(options: ClientOptions, customizer: &Customizer) -> NyquestResult<Self> {
        let inner = ReqwestClient::new(options, customizer)?;
        Ok(Self { inner })
    }
}

impl AsyncClient for ReqwestAsyncClient {
    type Response = ReqwestAsyncResponse;

    fn describe(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "ReqwestAsyncClient")
    }

    async fn request(&self, req: Request) -> NyquestResult<Self::Response> {
        #[cfg(feature = "async-stream")]
        let mut stream_task_collection = stream::StreamTaskCollection::default();
        let request_builder = self.inner.request(req, |stream| {
            #[cfg(feature = "async-stream")]
            {
                use nyquest_interface::r#async::BoxedStream;
                let size = match &stream {
                    BoxedStream::Sized { content_length, .. } => Some(*content_length),
                    BoxedStream::Unsized { .. } => None,
                };
                let stream = stream_task_collection.add_stream(stream);
                (reqwest::Body::wrap(stream), size)
            }
            #[cfg(not(feature = "async-stream"))]
            {
                let _ = stream;
                unreachable!("async-stream feature is disabled")
            }
        })?;

        // Execute the request using shared runtime handling
        let req_task = pin!(execute_with_runtime_async(
            &self.inner.managed_runtime,
            || async {
                request_builder
                    .send()
                    .await
                    .map_err(ReqwestBackendError::Reqwest)
            }
        ));
        #[cfg(not(feature = "async-stream"))]
        let stream_task = std::future::pending::<()>();
        #[cfg(feature = "async-stream")]
        let stream_task = pin!(stream_task_collection.execute());
        let response = if let Either::Left((res, _)) = select(req_task, stream_task).await {
            res
        } else {
            unreachable!()
        };
        ReqwestAsyncResponse::new(
            response?,
            self.inner.max_response_buffer_size,
            self.inner.managed_runtime.clone(),
        )
        .await
        .map_err(Into::into)
    }

    #[cfg(feature = "ws")]
    async fn connect_websocket(
        &self,
        uri: Cow<'static, str>,
        protocols: Vec<Cow<'static, str>>,
    ) -> nyquest_interface::websocket::WebSocketResult<
        nyquest_interface::r#async::websocket::WebSocket,
    > {
        websocket::connect_async(&self.inner, uri, protocols).await
    }
}

/// Execute an async task with proper runtime handling for async context
async fn execute_with_runtime_async<F, Fut, T: Send + 'static>(
    managed_runtime: &ManagedRuntime,
    task: F,
) -> T
where
    F: FnOnce() -> Fut + Send + 'static,
    Fut: Future<Output = T> + Send,
{
    let runtime = managed_runtime.get_handle_for_async();
    runtime
        .spawn(async move { task().await })
        .await
        .expect("spawned task panicked from managed runtime")
}

pub struct ReqwestAsyncResponse {
    response: ReqwestResponse,
    runtime: Arc<ManagedRuntime>,
}

impl ReqwestAsyncResponse {
    async fn new(
        response: reqwest::Response,
        max_response_buffer_size: Option<u64>,
        runtime: Arc<ManagedRuntime>,
    ) -> Result<Self> {
        Ok(Self {
            response: ReqwestResponse::new(response, max_response_buffer_size),
            runtime,
        })
    }
}

impl AsyncResponse for ReqwestAsyncResponse {
    fn describe(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "ReqwestAsyncResponse(status: {})", self.status())
    }

    fn status(&self) -> u16 {
        self.response.status()
    }

    fn content_length(&self) -> Option<u64> {
        self.response.content_length()
    }

    fn get_header(&self, header: &str) -> NyquestResult<Vec<String>> {
        self.response.get_header(header)
    }

    async fn text(self: Pin<&mut Self>) -> NyquestResult<String> {
        #[cfg(feature = "charset")]
        {
            let encoding = self.response.get_best_encoding();
            let bytes = AsyncResponse::bytes(self).await?;
            let (text, _, _) = encoding.decode(&bytes);
            Ok(text.into_owned())
        }

        #[cfg(not(feature = "charset"))]
        {
            let bytes = AsyncResponse::bytes(self).await?;
            Ok(String::from_utf8_lossy(&bytes).into_owned())
        }
    }

    async fn bytes(mut self: Pin<&mut Self>) -> NyquestResult<Vec<u8>> {
        let Self {
            runtime, response, ..
        } = &mut *self;
        let mut task = pin!(response.collect_all_bytes());
        poll_fn(|cx| {
            let _enter = runtime.get_handle_for_async().enter();
            task.as_mut().poll(cx)
        })
        .await
    }
}

#[cfg(feature = "async-stream")]
impl futures::AsyncRead for ReqwestAsyncResponse {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut [u8],
    ) -> std::task::Poll<std::io::Result<usize>> {
        use std::task::{ready, Poll};

        loop {
            let written = self.response.write_to(buf)?;
            if written > 0 {
                return Poll::Ready(Ok(written));
            }
            let _enter = self.runtime.get_handle_for_async().enter();
            let received = ready!(self.response.poll_receive_data_frame_buffered(cx))?;
            if received == 0 {
                break Poll::Ready(Ok(0));
            }
        }
    }
}

impl nyquest_interface::r#async::AsyncBackend for crate::ReqwestBackend {
    type AsyncClient = ReqwestAsyncClient;

    async fn create_async_client(
        &self,
        options: nyquest_interface::client::ClientOptions,
    ) -> NyquestResult<Self::AsyncClient> {
        ReqwestAsyncClient::new(options, self.customizer())
    }
}
