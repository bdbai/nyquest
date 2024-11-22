use std::io;

use nyquest::r#async::backend::{AsyncBackend, AsyncResponse};
use nyquest::r#async::Request;
use nyquest::{client::ClientOptions, r#async::backend::AsyncClient};
use windows::core::Interface;
use windows::Foundation::Uri;
use windows::Web::Http::HttpCompletionOption;
use windows::Win32::System::WinRT::IBufferByteAccess;
use windows::{core::HSTRING, Web::Http::HttpClient};

mod iasync_ext;
mod iasync_like;

use crate::request::{create_body, create_request};
use crate::response::WinrtResponse;
use crate::uri::build_uri;
use crate::{client::WinrtClientExt, error::IntoNyquestResult};
use iasync_ext::IAsyncExt;

#[derive(Clone)]
pub struct WinrtAsyncBackend;
#[derive(Clone)]
pub struct WinrtAsyncClient {
    base_url: Option<HSTRING>,
    client: HttpClient,
}

impl WinrtAsyncBackend {
    pub fn create_client(&self, options: ClientOptions) -> io::Result<WinrtAsyncClient> {
        let base_url = options.base_url.as_ref().map(|s| HSTRING::from(s));
        let client = HttpClient::create(options)?;
        Ok(WinrtAsyncClient { base_url, client })
    }
}

impl WinrtAsyncClient {
    async fn send_request(&self, uri: &Uri, req: Request) -> io::Result<WinrtResponse> {
        let req_msg = create_request(uri, &req)?;
        // TODO: stream
        if let Some(body) = req.body {
            let body = create_body(&req_msg, body, &mut |_| unimplemented!())?;
            req_msg.SetContent(&body)?;
        }
        let res = self
            .client
            .SendRequestWithOptionAsync(&req_msg, HttpCompletionOption::ResponseHeadersRead)?
            .wait()?
            .await?;
        WinrtResponse::new(res)
    }
}

impl AsyncResponse for WinrtResponse {
    fn status(&self) -> u16 {
        self.status
    }

    fn content_length(&self) -> Option<u64> {
        self.content_length
    }

    fn get_header(&self, header: &str) -> nyquest::Result<Vec<String>> {
        self.get_header(header).into_nyquest_result()
    }

    async fn text(&mut self) -> nyquest::Result<String> {
        let task = self
            .response
            .Content()
            .into_nyquest_result()?
            .ReadAsStringAsync()
            .into_nyquest_result()?;
        Ok(task
            .wait()
            .into_nyquest_result()?
            .await
            .into_nyquest_result()?
            .to_string_lossy())
    }

    async fn bytes(&mut self) -> nyquest::Result<Vec<u8>> {
        let task = self
            .response
            .Content()
            .into_nyquest_result()?
            .ReadAsBufferAsync()
            .into_nyquest_result()?;
        let buf = task
            .wait()
            .into_nyquest_result()?
            .await
            .into_nyquest_result()?;
        let len = buf.Length().into_nyquest_result()?;
        let iba = buf.cast::<IBufferByteAccess>().into_nyquest_result()?;
        unsafe {
            let ptr = iba.Buffer().into_nyquest_result()?;
            let bytes = std::slice::from_raw_parts(ptr, len as usize);
            Ok(bytes.to_vec())
        }
    }
}

impl AsyncClient for WinrtAsyncClient {
    type Response = WinrtResponse;

    async fn request(&self, req: Request) -> nyquest::Result<Self::Response> {
        let uri =
            build_uri(&self.base_url, &req.relative_uri).map_err(|_| nyquest::Error::InvalidUrl)?;
        self.send_request(&uri, req).await.into_nyquest_result()
    }
}

impl AsyncBackend for WinrtAsyncBackend {
    type AsyncClient = WinrtAsyncClient;

    async fn create_async_client(
        &self,
        options: ClientOptions,
    ) -> nyquest::client::BuildClientResult<Self::AsyncClient> {
        Ok(self.create_client(options).into_nyquest_result()?)
    }
}
