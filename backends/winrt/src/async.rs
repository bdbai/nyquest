use std::io;
use std::pin::{pin, Pin};
#[cfg(feature = "async-progress")]
use std::sync::Arc;

use nyquest_interface::client::ClientOptions;
use nyquest_interface::r#async::{AsyncBackend, AsyncClient, AsyncResponse, Request};
#[cfg(feature = "async-progress")]
use nyquest_interface::ProgressReporter;
use nyquest_interface::Result as NyquestResult;
use windows::Web::Http::HttpCompletionOption;

#[cfg(feature = "async-stream")]
mod read_task;
#[cfg(feature = "async-stream")]
mod stream_content;
mod timer_ext;
#[cfg(feature = "async-ws")]
mod websocket;

use crate::client::WinrtClient;
use crate::error::IntoNyquestResult;
use crate::ibuffer::IBufferExt;
use crate::request::create_body;
use crate::response::WinrtResponse;
#[cfg(not(feature = "async-progress"))]
use crate::response_size_limiter::ResponseSizeLimiter;
use crate::timer::Timer;
use timer_ext::AsyncTimeoutExt;

pub struct WinrtAsyncResponse {
    inner: WinrtResponse,
    #[cfg(feature = "async-stream")]
    read_task: read_task::ReadTask,
}

impl crate::WinrtBackend {
    pub fn create_async_client(&self, options: ClientOptions) -> io::Result<WinrtClient> {
        WinrtClient::create(options)
    }
}

impl WinrtClient {
    async fn send_request_async(
        &self,
        req: Request,
        #[cfg(feature = "async-progress")] progress: Option<Arc<dyn ProgressReporter>>,
    ) -> NyquestResult<WinrtAsyncResponse> {
        #[cfg(feature = "async-progress")]
        let progress_sink = progress.map(|reporter| {
            use crate::response::WinrtProgressSink;

            let upload_total = match &req.body {
                Some(nyquest_interface::Body::Bytes { content, .. }) => {
                    let len = content.len() as u64;
                    if len == 0 {
                        None
                    } else {
                        Some(len)
                    }
                }
                _ => None,
            };
            WinrtProgressSink::new(reporter, upload_total)
        });
        let req_msg = self.create_request(&req)?;
        #[cfg(feature = "async-stream")]
        let mut stream_tasks = Default::default();
        #[cfg(not(feature = "async-stream"))]
        let stream_tasks = std::future::pending::<()>();
        if let Some(body) = req.body {
            let body = create_body(body, &mut |s| {
                #[cfg(feature = "async-stream")]
                {
                    stream_content::transform_stream(s, &mut stream_tasks)
                }
                #[cfg(not(feature = "async-stream"))]
                {
                    let _ = s;
                    unreachable!("async-stream feature is disabled")
                }
            })?;
            self.append_content_headers(&body, &req.additional_headers)?;
            req_msg.SetContent(&body).into_nyquest_result()?;
        }
        let mut timer = Timer::new(self.request_timeout);
        let res = {
            let send_op = self
                .client
                .SendRequestWithOptionAsync(&req_msg, HttpCompletionOption::ResponseHeadersRead)
                .into_nyquest_result()?;
            #[cfg(feature = "async-progress")]
            if let Some(sink) = &progress_sink {
                crate::response::hook_send_progress(&send_op, sink)?;
            }
            let request_fut = pin!(send_op.timeout_by(&mut timer));
            match futures_util::future::select(request_fut, stream_tasks).await {
                futures_util::future::Either::Left((res, _)) => res,
                futures_util::future::Either::Right((_, _)) => unreachable!(),
            }?
        };
        // `mut` is only needed when the progress sink is attached below.
        #[allow(unused_mut)]
        let mut inner =
            WinrtResponse::new(res, self.max_response_buffer_size, timer).into_nyquest_result()?;
        #[cfg(feature = "async-progress")]
        if let Some(sink) = &progress_sink {
            sink.set_body_total(inner.content_length);
        }
        #[cfg(feature = "async-progress")]
        {
            inner.progress_sink = progress_sink;
        }
        Ok(WinrtAsyncResponse {
            #[cfg(feature = "async-stream")]
            read_task: read_task::ReadTask::new(inner.content().as_ref().ok()),
            inner,
        })
    }
}

impl AsyncResponse for WinrtAsyncResponse {
    fn status(&self) -> u16 {
        self.inner.status
    }

    fn content_length(&self) -> Option<u64> {
        self.inner.content_length
    }

    fn get_header(&self, header: &str) -> nyquest_interface::Result<Vec<String>> {
        self.inner.get_header(header).into_nyquest_result()
    }

    async fn text(mut self: Pin<&mut Self>) -> nyquest_interface::Result<String> {
        let task = self
            .inner
            .response
            .Content()
            .into_nyquest_result()?
            .ReadAsStringAsync()
            .into_nyquest_result()?;
        #[cfg(feature = "async-progress")]
        let progress_guard = crate::response::hook_read_task_progress(
            &task,
            self.inner.progress_sink.as_ref(),
            self.inner.max_response_buffer_size,
        )?;
        #[cfg(not(feature = "async-progress"))]
        let size_limiter =
            ResponseSizeLimiter::hook_progress(self.inner.max_response_buffer_size, &task)?;
        let res = task
            .timeout_by(&mut self.inner.request_timer)
            .await
            .map(|r| r.to_string_lossy());
        #[cfg(feature = "async-progress")]
        let content = progress_guard.assert_size(res)?;
        #[cfg(not(feature = "async-progress"))]
        let content = size_limiter.assert_size(res)?;
        Ok(content)
    }

    async fn bytes(mut self: Pin<&mut Self>) -> nyquest_interface::Result<Vec<u8>> {
        let task = self
            .inner
            .response
            .Content()
            .into_nyquest_result()?
            .ReadAsBufferAsync()
            .into_nyquest_result()?;
        #[cfg(feature = "async-progress")]
        let progress_guard = crate::response::hook_read_task_progress(
            &task,
            self.inner.progress_sink.as_ref(),
            self.inner.max_response_buffer_size,
        )?;
        #[cfg(not(feature = "async-progress"))]
        let size_limiter =
            ResponseSizeLimiter::hook_progress(self.inner.max_response_buffer_size, &task)?;
        let res = task
            .timeout_by(&mut self.inner.request_timer)
            .await
            .and_then(|b| b.to_vec());
        #[cfg(feature = "async-progress")]
        let arr = progress_guard.assert_size(res)?;
        #[cfg(not(feature = "async-progress"))]
        let arr = size_limiter.assert_size(res)?;
        Ok(arr)
    }
}

#[cfg(feature = "async-stream")]
impl nyquest_interface::r#async::futures_io::AsyncRead for WinrtAsyncResponse {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut [u8],
    ) -> std::task::Poll<io::Result<usize>> {
        let read_len = std::task::ready!(Pin::new(&mut self.read_task).poll_read(cx, buf))?;
        #[cfg(feature = "async-progress")]
        if read_len > 0 {
            if let Some(sink) = self.inner.progress_sink.as_ref() {
                sink.on_body_chunk(read_len as u64);
            }
        }
        std::task::Poll::Ready(Ok(read_len))
    }
}

impl AsyncClient for WinrtClient {
    type Response = WinrtAsyncResponse;

    async fn request(&self, req: Request) -> nyquest_interface::Result<Self::Response> {
        #[cfg(feature = "async-progress")]
        return self.send_request_async(req, None).await;
        #[cfg(not(feature = "async-progress"))]
        return self.send_request_async(req).await;
    }

    #[cfg(feature = "async-progress")]
    async fn request_with_progress(
        &self,
        req: Request,
        progress: Arc<dyn ProgressReporter>,
    ) -> NyquestResult<Self::Response> {
        self.send_request_async(req, Some(progress)).await
    }

    #[cfg(feature = "async-ws")]
    async fn connect_websocket(
        &self,
        uri: std::borrow::Cow<'static, str>,
        protocols: Vec<std::borrow::Cow<'static, str>>,
    ) -> nyquest_interface::websocket::WebSocketResult<
        nyquest_interface::r#async::websocket::WebSocket,
    > {
        websocket::connect_async(self, uri, protocols).await
    }

    #[cfg(feature = "cookies")]
    async fn add_cookie(
        &self,
        name: std::borrow::Cow<'static, str>,
        value: std::borrow::Cow<'static, str>,
        url: std::borrow::Cow<'static, str>,
    ) -> NyquestResult<()> {
        self.add_cookie_impl(&name, &value, &url)
    }

    #[cfg(feature = "cookies")]
    async fn get_cookies_for(
        &self,
        url: std::borrow::Cow<'static, str>,
    ) -> NyquestResult<Vec<nyquest_interface::Cookie>> {
        self.get_cookies_for_impl(&url)
    }
}

impl AsyncBackend for crate::WinrtBackend {
    type AsyncClient = WinrtClient;

    async fn create_async_client(
        &self,
        options: ClientOptions,
    ) -> NyquestResult<Self::AsyncClient> {
        self.create_async_client(options).into_nyquest_result()
    }
}
