use std::future::{poll_fn, Future};
use std::io;
use std::pin::{pin, Pin};
use std::sync::OnceLock;
use std::task::{ready, Context, Poll};

use futures::AsyncRead;
use nyquest_interface::client::ClientOptions;
use nyquest_interface::r#async::{AsyncClient, AsyncResponse, Request};
use nyquest_interface::Result as NyquestResult;
use tokio::runtime::{Handle, Runtime};

use crate::client::ReqwestClient;
use crate::error::{ReqwestBackendError, Result};
use crate::response::ReqwestResponse;

#[derive(Clone)]
pub struct ReqwestAsyncClient {
    inner: ReqwestClient,
}

impl ReqwestAsyncClient {
    pub fn new(options: ClientOptions) -> NyquestResult<Self> {
        let inner = ReqwestClient::new(options)?;
        Ok(Self { inner })
    }
}

impl AsyncClient for ReqwestAsyncClient {
    type Response = ReqwestAsyncResponse;

    fn describe(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "ReqwestAsyncClient")
    }

    async fn request(&self, req: Request) -> NyquestResult<Self::Response> {
        let request_builder = crate::request::build_request_generic(
            &self.inner.client,
            self.inner.base_url.as_ref(),
            req,
            |_body| unimplemented!(),
        )?;

        // Execute the request using shared runtime handling
        let (response, handle) =
            execute_with_runtime_async(&self.inner.managed_runtime, || async {
                request_builder
                    .send()
                    .await
                    .map_err(ReqwestBackendError::Reqwest)
            })
            .await;

        ReqwestAsyncResponse::new(response?, self.inner.max_response_buffer_size, handle)
            .await
            .map_err(Into::into)
    }
}

/// Create a new tokio runtime for async operations
fn create_managed_runtime() -> Runtime {
    tokio::runtime::Builder::new_multi_thread()
        .thread_name("nyquest-reqwest-async")
        .worker_threads(1)
        .enable_all()
        .build()
        .expect("Failed to create managed tokio runtime")
}

/// Execute an async task with proper runtime handling for async context
async fn execute_with_runtime_async<F, Fut, T: Send + 'static>(
    managed_runtime: &OnceLock<Runtime>,
    task: F,
) -> (T, Handle)
where
    F: FnOnce() -> Fut + Send + 'static,
    Fut: Future<Output = T> + Send,
{
    if let Ok(handle) = Handle::try_current() {
        // Inside tokio runtime - proceed normally
        (task().await, handle)
    } else {
        // Outside tokio runtime - use managed runtime with block_on
        let runtime = managed_runtime.get_or_init(create_managed_runtime);
        runtime
            .spawn(async {
                let handle = Handle::current();
                let result = task().await;
                (result, handle)
            })
            .await
            .expect("spawned task panicked from managed runtime")
    }
}

pub struct ReqwestAsyncResponse {
    response: ReqwestResponse,
    current_handle: Handle,
}

impl ReqwestAsyncResponse {
    async fn new(
        response: reqwest::Response,
        max_response_buffer_size: Option<u64>,
        current_handle: Handle,
    ) -> Result<Self> {
        Ok(Self {
            response: ReqwestResponse::new(response, max_response_buffer_size),
            current_handle,
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
            current_handle,
            response,
        } = &mut *self;
        let mut task = pin!(response.collect_all_bytes());
        poll_fn(|cx| {
            let _enter = current_handle.enter();
            task.as_mut().poll(cx)
        })
        .await
    }
}

impl AsyncRead for ReqwestAsyncResponse {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut [u8],
    ) -> Poll<io::Result<usize>> {
        loop {
            let written = self.response.write_to(buf)?;
            if written > 0 {
                return Poll::Ready(Ok(written));
            }
            let _enter = self.current_handle.enter();
            let received = ready!(self.response.poll_receive_data_frame_buffered(cx))?;
            if received == 0 {
                break Poll::Ready(Ok(0));
            }
        }
    }
}

impl nyquest_interface::r#async::AsyncBackend for ReqwestBackend {
    type AsyncClient = r#async::ReqwestAsyncClient;

    async fn create_async_client(
        &self,
        options: nyquest_interface::client::ClientOptions,
    ) -> Result<Self::AsyncClient> {
        r#async::ReqwestAsyncClient::new(options)
    }
}
