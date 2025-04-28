use std::io;

use nyquest_interface::client::ClientOptions;
use nyquest_interface::r#async::{AsyncBackend, AsyncClient, AsyncResponse, Request};
use nyquest_interface::Result as NyquestResult;
use windows::Web::Http::HttpCompletionOption;

use crate::client::WinrtClient;
use crate::error::IntoNyquestResult;
use crate::ibuffer::IBufferExt;
use crate::request::create_body;
use crate::response::WinrtResponse;
use crate::response_size_limiter::ResponseSizeLimiter;

impl crate::WinrtBackend {
    pub fn create_async_client(&self, options: ClientOptions) -> io::Result<WinrtClient> {
        WinrtClient::create(options)
    }
}

impl WinrtClient {
    async fn send_request_async(&self, req: Request) -> NyquestResult<WinrtResponse> {
        let req_msg = self.create_request(&req)?;
        // TODO: stream
        if let Some(body) = req.body {
            let body = create_body(body, &mut |_| unimplemented!())?;
            self.append_content_headers(&body, &req.additional_headers)?;
            req_msg.SetContent(&body).into_nyquest_result()?;
        }
        let res = self
            .client
            .SendRequestWithOptionAsync(&req_msg, HttpCompletionOption::ResponseHeadersRead)
            .into_nyquest_result()?
            .await
            .into_nyquest_result()?;
        WinrtResponse::new(res, self.max_response_buffer_size).into_nyquest_result()
    }
}

impl AsyncResponse for WinrtResponse {
    fn status(&self) -> u16 {
        self.status
    }

    fn content_length(&self) -> Option<u64> {
        self.content_length
    }

    fn get_header(&self, header: &str) -> nyquest_interface::Result<Vec<String>> {
        self.get_header(header).into_nyquest_result()
    }

    async fn text(&mut self) -> nyquest_interface::Result<String> {
        let task = self
            .response
            .Content()
            .into_nyquest_result()?
            .ReadAsStringAsync()
            .into_nyquest_result()?;
        let size_limiter =
            ResponseSizeLimiter::hook_progress(self.max_response_buffer_size, &task)?;
        let res = task
            .await
            .into_nyquest_result()
            .map(|r| r.to_string_lossy());
        let content = size_limiter.assert_size(res)?;
        Ok(content)
    }

    async fn bytes(&mut self) -> nyquest_interface::Result<Vec<u8>> {
        let task = self
            .response
            .Content()
            .into_nyquest_result()?
            .ReadAsBufferAsync()
            .into_nyquest_result()?;
        let size_limiter =
            ResponseSizeLimiter::hook_progress(self.max_response_buffer_size, &task)?;
        let res = task.await.into_nyquest_result().and_then(|b| b.to_vec());
        let arr = size_limiter.assert_size(res)?;
        Ok(arr)
    }
}

impl AsyncClient for WinrtClient {
    type Response = WinrtResponse;

    async fn request(&self, req: Request) -> nyquest_interface::Result<Self::Response> {
        self.send_request_async(req).await
    }
}

impl AsyncBackend for crate::WinrtBackend {
    type AsyncClient = WinrtClient;

    async fn create_async_client(
        &self,
        options: ClientOptions,
    ) -> nyquest_interface::client::BuildClientResult<Self::AsyncClient> {
        Ok(self.create_async_client(options).into_nyquest_result()?)
    }
}
