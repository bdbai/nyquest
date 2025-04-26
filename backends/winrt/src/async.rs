use std::io;

use nyquest_interface::client::ClientOptions;
use nyquest_interface::r#async::{AsyncBackend, AsyncClient, AsyncResponse, Request};
use windows::core::{Interface, HSTRING};
use windows::Foundation::Uri;
use windows::Web::Http::{HttpClient, HttpCompletionOption};
use windows::Win32::System::WinRT::IBufferByteAccess;

use crate::client::WinrtClientExt;
use crate::error::IntoNyquestResult;
use crate::request::{create_body, create_request};
use crate::response::WinrtResponse;
use crate::uri::build_uri;

#[derive(Clone)]
pub struct WinrtAsyncClient {
    base_url: Option<HSTRING>,
    client: HttpClient,
}

impl crate::WinrtBackend {
    pub fn create_async_client(&self, options: ClientOptions) -> io::Result<WinrtAsyncClient> {
        let base_url = options.base_url.as_ref().map(HSTRING::from);
        let client = HttpClient::create(options)?;
        Ok(WinrtAsyncClient { base_url, client })
    }
}

impl WinrtAsyncClient {
    async fn send_request(&self, uri: &Uri, req: Request) -> io::Result<WinrtResponse> {
        let req_msg = create_request(uri, &req)?;
        // TODO: stream
        if let Some(body) = req.body {
            let body = create_body(body, &mut |_| unimplemented!())?;
            req_msg.SetContent(&body)?;
        }
        let res = self
            .client
            .SendRequestWithOptionAsync(&req_msg, HttpCompletionOption::ResponseHeadersRead)?
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
        Ok(task.await.into_nyquest_result()?.to_string_lossy())
    }

    async fn bytes(&mut self) -> nyquest_interface::Result<Vec<u8>> {
        let task = self
            .response
            .Content()
            .into_nyquest_result()?
            .ReadAsBufferAsync()
            .into_nyquest_result()?;
        let buf = task.await.into_nyquest_result()?;
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

    async fn request(&self, req: Request) -> nyquest_interface::Result<Self::Response> {
        let uri = build_uri(&self.base_url, &req.relative_uri)
            .map_err(|_| nyquest_interface::Error::InvalidUrl)?;
        self.send_request(&uri, req).await.into_nyquest_result()
    }
}

impl AsyncBackend for crate::WinrtBackend {
    type AsyncClient = WinrtAsyncClient;

    async fn create_async_client(
        &self,
        options: ClientOptions,
    ) -> nyquest_interface::client::BuildClientResult<Self::AsyncClient> {
        Ok(self.create_async_client(options).into_nyquest_result()?)
    }
}
