#[cfg(any(feature = "blocking-ws", feature = "cookies"))]
use std::borrow::Cow;
#[cfg(feature = "blocking-stream")]
use std::io::{self, Read};
use std::sync::Arc;

use nyquest_interface::blocking::{BlockingClient, BlockingResponse, Request};
use nyquest_interface::client::ClientOptions;
use nyquest_interface::Result as NyquestResult;
use tokio::runtime::Handle;

use crate::client::ReqwestClient;
use crate::customizer::Customizer;
use crate::error::ReqwestBackendError;
use crate::response::ReqwestResponse;
use crate::runtime::ManagedRuntime;

#[cfg(feature = "blocking-stream")]
mod stream;
#[cfg(feature = "blocking-ws")]
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
        #[cfg(feature = "blocking-progress")]
        {
            struct Noop;
            impl nyquest_interface::ProgressReporter for Noop {
                fn report(&self, _progress: nyquest_interface::Progress) {}
            }
            self.request_with_progress(req, Arc::new(Noop))
        }
        #[cfg(not(feature = "blocking-progress"))]
        {
            let handle = self.inner.managed_runtime.get_handle_for_blocking();
            block_on_with_handle(&handle, || execute_request(self, req))
        }
    }

    #[cfg(feature = "blocking-progress")]
    fn request_with_progress(
        &self,
        req: Request,
        progress: Arc<dyn nyquest_interface::ProgressReporter>,
    ) -> NyquestResult<Self::Response> {
        let handle = self.inner.managed_runtime.get_handle_for_blocking();
        block_on_with_handle(&handle, || {
            execute_request_with_progress(self, req, progress)
        })
    }

    #[cfg(feature = "blocking-ws")]
    fn connect_websocket(
        &self,
        uri: Cow<'static, str>,
        protocols: Vec<Cow<'static, str>>,
    ) -> nyquest_interface::websocket::WebSocketResult<
        nyquest_interface::blocking::websocket::WebSocket,
    > {
        websocket::connect_blocking(&self.inner, uri, protocols)
    }

    #[cfg(all(feature = "cookies", not(target_arch = "wasm32")))]
    fn add_cookie(
        &self,
        name: Cow<'static, str>,
        value: Cow<'static, str>,
        url: Cow<'static, str>,
    ) -> NyquestResult<()> {
        self.inner.add_cookie(&name, &value, &url)
    }

    #[cfg(all(feature = "cookies", not(target_arch = "wasm32")))]
    fn get_cookies_for(
        &self,
        url: Cow<'static, str>,
    ) -> NyquestResult<Vec<nyquest_interface::Cookie>> {
        self.inner.get_cookies_for(&url)
    }
}

#[cfg(not(feature = "blocking-progress"))]
async fn execute_request(
    this: &ReqwestBlockingClient,
    req: Request,
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
        this.inner.managed_runtime.clone(),
    )?)
}

#[cfg(feature = "blocking-progress")]
async fn execute_request_with_progress(
    this: &ReqwestBlockingClient,
    req: Request,
    progress: Arc<dyn nyquest_interface::ProgressReporter>,
) -> NyquestResult<ReqwestBlockingResponse> {
    use crate::progress::{upload_total_for_blocking_body, ProgressTracker};
    let upload_total = upload_total_for_blocking_body(&req.body);
    let tracker = ProgressTracker::new(progress, upload_total);
    #[cfg(feature = "blocking-stream")]
    let tracker_for_streams = tracker.clone();
    let tracker_for_bytes = tracker.clone();
    let request_builder = this.inner.request_with_bytes_progress(
        req,
        |stream| {
            #[cfg(feature = "blocking-stream")]
            {
                use nyquest_interface::blocking::BoxedStream;
                let size = match &stream {
                    BoxedStream::Sized { content_length, .. } => Some(*content_length),
                    BoxedStream::Unsized { .. } => None,
                };
                (
                    reqwest::Body::wrap(stream::BlockingStreamBody::new_with_progress(
                        stream,
                        Some(tracker_for_streams.clone()),
                    )),
                    size,
                )
            }
            #[cfg(not(feature = "blocking-stream"))]
            {
                let _ = stream;
                unreachable!("blocking-stream feature is disabled")
            }
        },
        Some(&tracker_for_bytes),
    )?;

    let response = request_builder
        .send()
        .await
        .map_err(ReqwestBackendError::Reqwest)?;

    Ok(ReqwestBlockingResponse::new_with_progress(
        response,
        this.inner.max_response_buffer_size,
        this.inner.managed_runtime.clone(),
        tracker,
    )?)
}

pub struct ReqwestBlockingResponse {
    response: ReqwestResponse,
    managed_runtime: Arc<ManagedRuntime>,
}

impl ReqwestBlockingResponse {
    #[cfg(not(feature = "blocking-progress"))]
    fn new(
        response: reqwest::Response,
        max_response_buffer_size: Option<u64>,
        managed_runtime: Arc<ManagedRuntime>,
    ) -> crate::error::Result<Self> {
        let response = ReqwestResponse::new(response, max_response_buffer_size);

        Ok(Self {
            response,
            managed_runtime,
        })
    }

    #[cfg(feature = "blocking-progress")]
    fn new_with_progress(
        response: reqwest::Response,
        max_response_buffer_size: Option<u64>,
        managed_runtime: Arc<ManagedRuntime>,
        tracker: Arc<crate::progress::ProgressTracker>,
    ) -> crate::error::Result<Self> {
        let mut inner = ReqwestResponse::new(response, max_response_buffer_size);
        inner.set_progress(tracker);
        Ok(Self {
            response: inner,
            managed_runtime,
        })
    }
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
        let handle = self.managed_runtime.get_handle_for_blocking();
        block_on_with_handle(&handle, || self.response.collect_all_bytes())
    }
}

#[cfg(feature = "blocking-stream")]
impl Read for ReqwestBlockingResponse {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let handle = self.managed_runtime.get_handle_for_blocking();
        loop {
            let written = self.response.write_to(buf)?;
            if written > 0 {
                break Ok(written);
            }
            let received =
                block_on_with_handle(&handle, || self.response.receive_data_frame_buffered())?;
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
