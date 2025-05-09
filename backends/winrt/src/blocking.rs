use std::io;

use nyquest_interface::blocking::{BlockingBackend, BlockingClient, BlockingResponse, Request};
use nyquest_interface::client::{BuildClientResult, ClientOptions};
use nyquest_interface::Result as NyquestResult;
use timer_ext::BlockingTimeoutExt;
use windows::Web::Http::HttpCompletionOption;

mod timer_ext;

use crate::client::WinrtClient;
use crate::error::IntoNyquestResult;
use crate::ibuffer::IBufferExt;
use crate::request::create_body;
use crate::response::WinrtResponse;
use crate::response_size_limiter::ResponseSizeLimiter;
use crate::timer::Timer;

pub struct WinrtBlockingResponse {
    inner: WinrtResponse,
}

impl crate::WinrtBackend {
    pub fn create_blocking_client(&self, options: ClientOptions) -> io::Result<WinrtClient> {
        WinrtClient::create(options)
    }
}

impl WinrtClient {
    fn send_request(&self, req: Request) -> NyquestResult<WinrtBlockingResponse> {
        let req_msg = self.create_request(&req)?;
        // TODO: stream
        if let Some(body) = req.body {
            let body = create_body(body, &mut |_| unimplemented!())?;
            self.append_content_headers(&body, &req.additional_headers)?;
            req_msg.SetContent(&body).into_nyquest_result()?;
        }
        let mut timer = Timer::new(self.request_timeout);
        let res = self
            .client
            .SendRequestWithOptionAsync(&req_msg, HttpCompletionOption::ResponseHeadersRead)
            .into_nyquest_result()?
            .timeout_by(&mut timer)?;
        let inner =
            WinrtResponse::new(res, self.max_response_buffer_size, timer).into_nyquest_result()?;
        Ok(WinrtBlockingResponse { inner })
    }
}

impl BlockingClient for WinrtClient {
    type Response = WinrtBlockingResponse;
    fn request(&self, req: Request) -> NyquestResult<Self::Response> {
        self.send_request(req)
    }
}

impl BlockingBackend for crate::WinrtBackend {
    type BlockingClient = WinrtClient;

    fn create_blocking_client(
        &self,
        options: ClientOptions,
    ) -> BuildClientResult<Self::BlockingClient> {
        Ok(self.create_blocking_client(options).into_nyquest_result()?)
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
        let size_limiter =
            ResponseSizeLimiter::hook_progress(self.inner.max_response_buffer_size, &task)?;
        let res = task
            .timeout_by(&mut self.inner.request_timer)
            .map(|r| r.to_string_lossy());
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
        let size_limiter =
            ResponseSizeLimiter::hook_progress(self.inner.max_response_buffer_size, &task)?;
        let res = task
            .timeout_by(&mut self.inner.request_timer)
            .and_then(|b| b.to_vec());
        let arr = size_limiter.assert_size(res)?;
        Ok(arr)
    }
}

impl io::Read for WinrtBlockingResponse {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let reader = self.inner.reader_mut()?;

        let mut size = reader.UnconsumedBufferLength()?;
        if size == 0 {
            let loaded = reader.LoadAsync(buf.len() as u32)?.get()?;
            if loaded == 0 {
                return Ok(0);
            }
            size = reader.UnconsumedBufferLength()?;
        }
        let size = buf.len().min(size as usize);
        let buf = &mut buf[..size];
        reader.ReadBytes(buf)?;
        Ok(size)
    }
}
