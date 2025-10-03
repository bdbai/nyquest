use std::{
    io,
    pin::Pin,
    task::{Context, Poll},
};

use futures::{AsyncRead, StreamExt};
use nyquest_interface::r#async::{AsyncBackend, AsyncClient, AsyncResponse};
use reqwest::Response as ReqwestResponse;

use crate::{client::ReqwestClient, error::ReqwestBackendError, wasm::send_wrapper::SendWrapper};

#[derive(Clone)]
pub struct ReqwestAsyncClient {
    inner: ReqwestClient,
}
pub struct ReqwestAsyncResponse {
    response: Option<SendWrapper<ReqwestResponse>>,
    max_response_buffer_size: Option<u64>,
}

fn bail_unimplemented() -> ! {
    unimplemented!("blocking backend should not be used in wasm32 target");
}

impl ReqwestAsyncResponse {
    fn response_ref(&self) -> &ReqwestResponse {
        self.response.as_ref().expect("response already consumed")
    }
}

impl AsyncRead for ReqwestAsyncResponse {
    fn poll_read(
        self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
        _buf: &mut [u8],
    ) -> Poll<io::Result<usize>> {
        bail_unimplemented()
    }
}

impl AsyncResponse for ReqwestAsyncResponse {
    fn status(&self) -> u16 {
        self.response_ref().status().as_u16()
    }

    fn content_length(&self) -> Option<u64> {
        self.response_ref().content_length()
    }

    fn get_header(&self, header: &str) -> nyquest_interface::Result<Vec<String>> {
        Ok(self
            .response_ref()
            .headers()
            .get_all(header)
            .iter()
            .map(|v| v.to_str().ok().unwrap_or_default().into())
            .collect())
    }

    async fn text(self: Pin<&mut Self>) -> nyquest_interface::Result<String> {
        let content_type = self
            .get_header("content-type")?
            .into_iter()
            .next()
            .unwrap_or_default();
        let charset = content_type
            .split(';')
            .find_map(|s| s.trim().strip_prefix("charset="));
        let bytes = self.bytes().await?;
        Ok(
            iconv_native::decode_lossy(&bytes, charset.unwrap_or("utf-8"))
                .map_err(|_| ReqwestBackendError::UnknownCharset)?,
        )
    }

    async fn bytes(mut self: Pin<&mut Self>) -> nyquest_interface::Result<Vec<u8>> {
        let mut bufs = vec![];
        let mut collected_size = 0;
        let mut stream = SendWrapper::new(
            self.as_mut()
                .response
                .take()
                .expect("cannot consume response more than once")
                .into_inner()
                .bytes_stream(),
        );
        loop {
            let Some(frame) = SendWrapper::new(stream.next()).await else {
                break;
            };
            let frame = frame.map_err(ReqwestBackendError::Reqwest)?;
            if self
                .max_response_buffer_size
                .is_some_and(|max| collected_size + frame.len() > max as usize)
            {
                return Err(ReqwestBackendError::ResponseTooLarge.into());
            }
            collected_size += frame.len();
            bufs.push(frame);
        }
        Ok(bufs.concat())
    }
}

impl AsyncClient for ReqwestAsyncClient {
    type Response = ReqwestAsyncResponse;

    async fn request(
        &self,
        req: nyquest_interface::r#async::Request,
    ) -> nyquest_interface::Result<Self::Response> {
        let request_builder = self.inner.request(req, |_body| unimplemented!())?;

        // Execute the request using shared runtime handling
        let response = SendWrapper::new(request_builder.send())
            .await
            .map_err(ReqwestBackendError::Reqwest)?;

        Ok(ReqwestAsyncResponse {
            response: Some(SendWrapper::new(response)),
            max_response_buffer_size: self.inner.max_response_buffer_size,
        })
    }
}

impl AsyncBackend for crate::ReqwestBackend {
    type AsyncClient = ReqwestAsyncClient;

    async fn create_async_client(
        &self,
        options: nyquest_interface::client::ClientOptions,
    ) -> nyquest_interface::Result<Self::AsyncClient> {
        let inner = ReqwestClient::new(options)?;
        Ok(ReqwestAsyncClient { inner })
    }
}
