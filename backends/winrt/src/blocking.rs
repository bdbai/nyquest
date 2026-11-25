use std::io;
#[cfg(feature = "blocking-progress")]
use std::sync::Arc;

use nyquest_interface::blocking::{BlockingBackend, BlockingClient, BlockingResponse, Request};
use nyquest_interface::client::ClientOptions;
#[cfg(feature = "blocking-progress")]
use nyquest_interface::ProgressReporter;
use nyquest_interface::Result as NyquestResult;
use timer_ext::BlockingTimeoutExt;
use windows::Storage::Streams::{DataReader, InputStreamOptions};
use windows::Web::Http::HttpCompletionOption;

#[cfg(feature = "blocking-stream")]
mod stream_content;
mod timer_ext;
#[cfg(feature = "blocking-ws")]
mod websocket;

use crate::client::WinrtClient;
use crate::error::IntoNyquestResult;
use crate::ibuffer::IBufferExt;
use crate::request::create_body;
use crate::response::WinrtResponse;
#[cfg(not(feature = "blocking-progress"))]
use crate::response_size_limiter::ResponseSizeLimiter;
use crate::timer::Timer;

pub struct WinrtBlockingResponse {
    inner: WinrtResponse,
    reader: Option<DataReader>,
}

impl crate::WinrtBackend {
    pub fn create_blocking_client(&self, options: ClientOptions) -> io::Result<WinrtClient> {
        WinrtClient::create(options)
    }
}

impl WinrtClient {
    fn send_request(
        &self,
        req: Request,
        #[cfg(feature = "blocking-progress")] progress: Option<Arc<dyn ProgressReporter>>,
    ) -> NyquestResult<WinrtBlockingResponse> {
        #[cfg(feature = "blocking-progress")]
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
        if let Some(body) = req.body {
            #[cfg(feature = "blocking-stream")]
            let body = create_body(body, &mut stream_content::transform_stream)?;
            #[cfg(not(feature = "blocking-stream"))]
            let body = create_body(body, &mut |_| {
                unreachable!("blocking-stream feature is disabled")
            })?;
            self.append_content_headers(&body, &req.additional_headers)?;
            req_msg.SetContent(&body).into_nyquest_result()?;
        }
        let mut timer = Timer::new(self.request_timeout);
        let send_op = self
            .client
            .SendRequestWithOptionAsync(&req_msg, HttpCompletionOption::ResponseHeadersRead)
            .into_nyquest_result()?;
        #[cfg(feature = "blocking-progress")]
        if let Some(sink) = &progress_sink {
            crate::response::hook_send_progress(&send_op, sink)?;
        }
        let res = send_op.timeout_by(&mut timer)?;
        // `mut` is only needed when the progress sink is attached below.
        #[allow(unused_mut)]
        let mut inner =
            WinrtResponse::new(res, self.max_response_buffer_size, timer).into_nyquest_result()?;
        #[cfg(feature = "blocking-progress")]
        if let Some(sink) = &progress_sink {
            sink.set_body_total(inner.content_length);
        }
        #[cfg(feature = "blocking-progress")]
        {
            inner.progress_sink = progress_sink;
        }
        Ok(WinrtBlockingResponse {
            inner,
            reader: None,
        })
    }
}

impl BlockingClient for WinrtClient {
    type Response = WinrtBlockingResponse;
    fn request(&self, req: Request) -> NyquestResult<Self::Response> {
        #[cfg(feature = "blocking-progress")]
        return self.send_request(req, None);
        #[cfg(not(feature = "blocking-progress"))]
        return self.send_request(req);
    }

    #[cfg(feature = "blocking-progress")]
    fn request_with_progress(
        &self,
        req: Request,
        progress: Arc<dyn ProgressReporter>,
    ) -> NyquestResult<Self::Response> {
        self.send_request(req, Some(progress))
    }

    #[cfg(feature = "blocking-ws")]
    fn connect_websocket(
        &self,
        uri: std::borrow::Cow<'static, str>,
        protocols: Vec<std::borrow::Cow<'static, str>>,
    ) -> nyquest_interface::websocket::WebSocketResult<
        nyquest_interface::blocking::websocket::WebSocket,
    > {
        websocket::connect_blocking(self, uri, protocols)
    }

    #[cfg(feature = "cookies")]
    fn add_cookie(
        &self,
        name: std::borrow::Cow<'static, str>,
        value: std::borrow::Cow<'static, str>,
        url: std::borrow::Cow<'static, str>,
    ) -> NyquestResult<()> {
        self.add_cookie_impl(&name, &value, &url)
    }

    #[cfg(feature = "cookies")]
    fn get_cookies_for(
        &self,
        url: std::borrow::Cow<'static, str>,
    ) -> NyquestResult<Vec<nyquest_interface::Cookie>> {
        self.get_cookies_for_impl(&url)
    }
}

impl BlockingBackend for crate::WinrtBackend {
    type BlockingClient = WinrtClient;

    fn create_blocking_client(
        &self,
        options: ClientOptions,
    ) -> NyquestResult<Self::BlockingClient> {
        self.create_blocking_client(options).into_nyquest_result()
    }
}

impl BlockingResponse for WinrtBlockingResponse {
    fn status(&self) -> u16 {
        self.inner.status
    }

    fn get_header(&self, header: &str) -> nyquest_interface::Result<Vec<String>> {
        self.inner.get_header(header).into_nyquest_result()
    }

    fn content_length(&self) -> Option<u64> {
        self.inner.content_length
    }

    fn text(&mut self) -> NyquestResult<String> {
        let task = self
            .inner
            .content()
            .into_nyquest_result()?
            .ReadAsStringAsync()
            .into_nyquest_result()?;
        #[cfg(feature = "blocking-progress")]
        let progress_guard = crate::response::hook_read_task_progress(
            &task,
            self.inner.progress_sink.as_ref(),
            self.inner.max_response_buffer_size,
        )?;
        #[cfg(not(feature = "blocking-progress"))]
        let size_limiter =
            ResponseSizeLimiter::hook_progress(self.inner.max_response_buffer_size, &task)?;
        let res = task
            .timeout_by(&mut self.inner.request_timer)
            .map(|r| r.to_string_lossy());
        #[cfg(feature = "blocking-progress")]
        let content = progress_guard.assert_size(res)?;
        #[cfg(not(feature = "blocking-progress"))]
        let content = size_limiter.assert_size(res)?;
        Ok(content)
    }

    fn bytes(&mut self) -> NyquestResult<Vec<u8>> {
        let task = self
            .inner
            .content()
            .into_nyquest_result()?
            .ReadAsBufferAsync()
            .into_nyquest_result()?;
        #[cfg(feature = "blocking-progress")]
        let progress_guard = crate::response::hook_read_task_progress(
            &task,
            self.inner.progress_sink.as_ref(),
            self.inner.max_response_buffer_size,
        )?;
        #[cfg(not(feature = "blocking-progress"))]
        let size_limiter =
            ResponseSizeLimiter::hook_progress(self.inner.max_response_buffer_size, &task)?;
        let res = task
            .timeout_by(&mut self.inner.request_timer)
            .and_then(|b| b.to_vec());
        #[cfg(feature = "blocking-progress")]
        let arr = progress_guard.assert_size(res)?;
        #[cfg(not(feature = "blocking-progress"))]
        let arr = size_limiter.assert_size(res)?;
        Ok(arr)
    }
}

#[cfg(feature = "blocking-stream")]
impl io::Read for WinrtBlockingResponse {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let reader = self.reader_mut()?;

        let mut size = reader.UnconsumedBufferLength()?;
        if size == 0 {
            let loaded = reader.LoadAsync(buf.len() as u32)?.join()?;
            if loaded == 0 {
                return Ok(0);
            }
            size = reader.UnconsumedBufferLength()?;
        }
        let size = buf.len().min(size as usize);
        let buf = &mut buf[..size];
        reader.ReadBytes(buf)?;
        #[cfg(feature = "blocking-progress")]
        if size > 0 {
            if let Some(sink) = self.inner.progress_sink.as_ref() {
                sink.on_body_chunk(size as u64);
            }
        }
        Ok(size)
    }
}

impl WinrtBlockingResponse {
    fn reader_mut(&mut self) -> io::Result<&mut DataReader> {
        if self.reader.is_none() {
            let content = self.inner.content()?;
            let content = content.ReadAsInputStreamAsync()?.join()?;
            let reader = DataReader::CreateDataReader(&content)?;
            reader.SetInputStreamOptions(InputStreamOptions::Partial)?;
            self.reader = Some(reader);
        }
        Ok(self.reader.as_mut().expect("DataReader is None"))
    }
}
