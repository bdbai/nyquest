use std::io::{self, Read};
use std::sync::{Arc, OnceLock};

use nyquest_interface::blocking::{BlockingClient, BlockingResponse, Request};
use nyquest_interface::client::ClientOptions;
use nyquest_interface::Result as NyquestResult;
use tokio::runtime::{Handle, Runtime};

use crate::client::ReqwestClient;
use crate::error::ReqwestBackendError;
use crate::response::ReqwestResponse;

#[derive(Clone)]
pub struct ReqwestBlockingClient {
    inner: ReqwestClient,
}

impl ReqwestBlockingClient {
    pub fn new(options: ClientOptions) -> NyquestResult<Self> {
        let inner = ReqwestClient::new(options)?;
        Ok(Self { inner })
    }
}

impl BlockingClient for ReqwestBlockingClient {
    type Response = ReqwestBlockingResponse;

    fn describe(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "ReqwestBlockingClient")
    }

    fn request(&self, req: Request) -> NyquestResult<Self::Response> {
        execute_with_runtime(&self.inner.managed_runtime, || async {
            execute_request(self, req).await
        })
    }
}

async fn execute_request(
    this: &ReqwestBlockingClient,
    req: Request,
) -> NyquestResult<ReqwestBlockingResponse> {
    let request_builder = crate::request::build_request_generic(
        &this.inner.client,
        this.inner.base_url.as_ref(),
        req,
        |_body| unimplemented!(),
    )?;

    let response = request_builder
        .send()
        .await
        .map_err(ReqwestBackendError::Reqwest)?;

    Ok(ReqwestBlockingResponse::new(
        response,
        this.inner.max_response_buffer_size,
        this.inner.managed_runtime.clone(),
    )?)
}

pub struct ReqwestBlockingResponse {
    response: ReqwestResponse,
    managed_runtime: Arc<OnceLock<Runtime>>,
}

impl ReqwestBlockingResponse {
    fn new(
        response: reqwest::Response,
        max_response_buffer_size: Option<u64>,
        managed_runtime: Arc<OnceLock<Runtime>>,
    ) -> crate::error::Result<Self> {
        let response = ReqwestResponse::new(response, max_response_buffer_size);

        Ok(Self {
            response,
            managed_runtime,
        })
    }
}

/// Create a new tokio runtime for blocking operations
fn create_managed_runtime() -> Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("Failed to create managed tokio runtime")
}

/// Execute an async task with proper runtime handling
fn execute_with_runtime<F, Fut, T>(managed_runtime: &OnceLock<Runtime>, task: F) -> T
where
    F: FnOnce() -> Fut,
    Fut: std::future::Future<Output = T>,
{
    if Handle::try_current().is_ok() {
        // Inside tokio runtime - use block_in_place + Handle::block_on
        tokio::task::block_in_place(|| Handle::current().block_on(task()))
    } else {
        // Outside tokio runtime - use managed runtime
        let runtime = managed_runtime.get_or_init(create_managed_runtime);
        runtime.block_on(task())
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
        execute_with_runtime(&self.managed_runtime, || self.response.collect_all_bytes())
    }
}

impl Read for ReqwestBlockingResponse {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        loop {
            let written = self.response.write_to(buf)?;
            if written > 0 {
                break Ok(written);
            }
            let received = execute_with_runtime(&self.managed_runtime, || {
                self.response.receive_data_frame_buffered()
            })?;
            if received == 0 {
                break Ok(0);
            }
        }
    }
}
