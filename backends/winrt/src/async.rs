use std::io;

use nyquest::r#async::backend::{AsyncBackend, AsyncResponse};
use nyquest::r#async::Body;
use nyquest::Request;
use nyquest::{client::ClientOptions, r#async::backend::AsyncClient};
use windows::Foundation::Uri;
use windows::Web::Http::{HttpMethod, HttpRequestMessage};
use windows::{core::HSTRING, Web::Http::HttpClient};

mod iasync_ext;
mod iasync_like;

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
    async fn send_request(&self, uri: &Uri, req: Request<Body>) -> io::Result<WinrtResponse> {
        // TODO: share code with blocking
        let method = HttpMethod::Create(&HSTRING::from(&*req.method))?;
        let req_msg = HttpRequestMessage::Create(&method, uri)?;
        // TODO: cache method
        req_msg.SetRequestUri(uri)?;
        // TODO: content
        let res = self.client.SendRequestAsync(&req_msg)?.wait()?.await?;
        let status = res.StatusCode()?.0 as u16;
        let content_length = match res.Content() {
            Ok(content) => content.Headers()?.ContentLength()?.Value().ok(),
            Err(_) => Some(0),
        };
        Ok(WinrtResponse {
            status,
            content_length,
            response: res,
            reader: None,
        })
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
        todo!()
    }
}

impl AsyncClient for WinrtAsyncClient {
    type Response = WinrtResponse;

    async fn request(
        &self,
        req: nyquest::Request<nyquest::r#async::Body>,
    ) -> nyquest::Result<Self::Response> {
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
