use std::borrow::Cow;
#[cfg(feature = "blocking-stream")]
use std::io::{self, Read};
use std::sync::{Arc, OnceLock};

use nyquest_interface::blocking::{BlockingClient, BlockingResponse, Request};
use nyquest_interface::client::ClientOptions;
use nyquest_interface::Result as NyquestResult;
use tokio::runtime::{Handle, Runtime};

use crate::client::ReqwestClient;
use crate::customizer::Customizer;
use crate::error::ReqwestBackendError;
use crate::response::ReqwestResponse;

#[cfg(feature = "blocking-stream")]
mod stream;
#[cfg(feature = "ws")]
mod websocket;

#[derive(Clone)]
pub struct ReqwestBlockingClient {
    inner: ReqwestClient,
}

impl ReqwestBlockingClient {
    pub(crate) fn new(options: ClientOptions, customizer: &Customizer) -> NyquestResult<Self> {
        let inner = ReqwestClient::new(options, customizer)?;
        Ok(Self { inner })
    }
}

impl BlockingClient for ReqwestBlockingClient {
    type Response = ReqwestBlockingResponse;

    fn describe(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "ReqwestBlockingClient")
    }

    fn request(&self, req: Request) -> NyquestResult<Self::Response> {
        let handle = select_runtime_handle(&self.inner.managed_runtime);
        block_on_with_handle(&handle, || execute_request(self, req, handle.clone()))
    }

    #[cfg(feature = "ws")]
    fn connect_websocket(
        &self,
        uri: Cow<'static, str>,
        protocols: Vec<Cow<'static, str>>,
    ) -> nyquest_interface::websocket::WebSocketResult<
        nyquest_interface::blocking::websocket::WebSocket,
    > {
        websocket::connect_blocking(&self.inner, uri, protocols)
    }
}

async fn execute_request(
    this: &ReqwestBlockingClient,
    req: Request,
    handle: Handle,
) -> NyquestResult<ReqwestBlockingResponse> {
    let request_builder = this.inner.request(req, |stream| {
        #[cfg(feature = "blocking-stream")]
        {
            use nyquest_interface::blocking::BoxedStream;
            let size = match &stream {
                BoxedStream::Sized { content_length, .. } => Some(*content_length),
                BoxedStream::Unsized { .. } => None,
            };
            (
                reqwest::Body::wrap(stream::BlockingStreamBody::new(stream)),
                size,
            )
        }
        #[cfg(not(feature = "blocking-stream"))]
        {
            let _ = stream;
            unreachable!("blocking-stream feature is disabled")
        }
    })?;

    let response = request_builder
        .send()
        .await
        .map_err(ReqwestBackendError::Reqwest)?;

    Ok(ReqwestBlockingResponse::new(
        response,
        this.inner.max_response_buffer_size,
        handle,
        this.inner.managed_runtime.clone(),
    )?)
}

pub struct ReqwestBlockingResponse {
    response: ReqwestResponse,
    handle: Handle,
    _managed_runtime: Arc<OnceLock<Runtime>>,
}

impl ReqwestBlockingResponse {
    fn new(
        response: reqwest::Response,
        max_response_buffer_size: Option<u64>,
        handle: Handle,
        managed_runtime: Arc<OnceLock<Runtime>>,
    ) -> crate::error::Result<Self> {
        let response = ReqwestResponse::new(response, max_response_buffer_size);

        Ok(Self {
            response,
            handle,
            _managed_runtime: managed_runtime,
        })
    }
}

/// Create a new tokio runtime for blocking operations
fn create_managed_runtime() -> Runtime {
    tokio::runtime::Builder::new_multi_thread()
        .thread_name("nyquest-reqwest-blocking")
        .worker_threads(1)
        .enable_all()
        .build()
        .expect("Failed to create managed tokio runtime")
}

pub(crate) fn select_runtime_handle(managed_runtime: &OnceLock<Runtime>) -> Handle {
    if let Ok(handle) = Handle::try_current() {
        if handle.runtime_flavor() == tokio::runtime::RuntimeFlavor::MultiThread {
            return handle;
        }
    }
    managed_runtime
        .get_or_init(create_managed_runtime)
        .handle()
        .clone()
}

pub(crate) fn block_on_with_handle<F, Fut, T>(handle: &Handle, task: F) -> T
where
    F: FnOnce() -> Fut,
    Fut: std::future::Future<Output = T>,
{
    let run = || {
        let _enter = handle.enter();
        let mut fut = std::pin::pin!(task());
        futures::executor::block_on(std::future::poll_fn(|cx| {
            let _enter = handle.enter();
            fut.as_mut().poll(cx)
        }))
    };
    if Handle::try_current()
        .is_ok_and(|h| h.runtime_flavor() == tokio::runtime::RuntimeFlavor::MultiThread)
    {
        tokio::task::block_in_place(run)
    } else {
        run()
    }
}

impl ReqwestBlockingResponse {}

impl BlockingResponse for ReqwestBlockingResponse {
    fn describe(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "ReqwestBlockingResponse(status: {})", self.status())
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

    fn text(&mut self) -> NyquestResult<String> {
        #[cfg(feature = "charset")]
        {
            let encoding = self.response.get_best_encoding();
            let bytes = BlockingResponse::bytes(self)?;
            let (text, _, _) = encoding.decode(&bytes);
            Ok(text.into_owned())
        }

        #[cfg(not(feature = "charset"))]
        {
            let bytes = BlockingResponse::bytes(self)?;
            Ok(String::from_utf8_lossy(&bytes).into_owned())
        }
    }

    fn bytes(&mut self) -> NyquestResult<Vec<u8>> {
        block_on_with_handle(&self.handle, || self.response.collect_all_bytes())
    }
}

#[cfg(feature = "blocking-stream")]
impl Read for ReqwestBlockingResponse {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        loop {
            let written = self.response.write_to(buf)?;
            if written > 0 {
                break Ok(written);
            }
            let received =
                block_on_with_handle(&self.handle, || self.response.receive_data_frame_buffered())?;
            if received == 0 {
                break Ok(0);
            }
        }
    }
}

impl nyquest_interface::blocking::BlockingBackend for crate::ReqwestBackend {
    type BlockingClient = ReqwestBlockingClient;

    fn create_blocking_client(
        &self,
        options: nyquest_interface::client::ClientOptions,
    ) -> NyquestResult<Self::BlockingClient> {
        ReqwestBlockingClient::new(options, self.customizer())
    }
}
