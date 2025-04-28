use std::sync::Arc;

use curl::easy::Easy;
use nyquest_interface::{client::BuildClientResult, r#async::AsyncResponse};

use crate::url::concat_url;

mod r#loop;

pub struct CurlMultiClientInner {
    options: nyquest_interface::client::ClientOptions,
    loop_manager: r#loop::LoopManager,
}
#[derive(Clone)]
pub struct CurlMultiClient {
    inner: Arc<CurlMultiClientInner>,
}

pub struct CurlAsyncResponse {
    status: u16,
    content_length: Option<u64>,
    headers: Vec<(String, String)>,
    handle: r#loop::RequestHandle,
    max_response_buffer_size: Option<u64>,
}

impl AsyncResponse for CurlAsyncResponse {
    fn status(&self) -> u16 {
        self.status
    }

    fn content_length(&self) -> Option<u64> {
        self.content_length
    }

    fn get_header(&self, header: &str) -> nyquest_interface::Result<Vec<String>> {
        Ok(self
            .headers
            .iter()
            .filter(|(k, _)| k.eq_ignore_ascii_case(header))
            .map(|(_, v)| v.clone())
            .collect())
    }

    async fn text(&mut self) -> nyquest_interface::Result<String> {
        let buf = self.bytes().await?;
        #[cfg(feature = "charset")]
        if let Some((_, mut charset)) = self
            .get_header("content-type")?
            .pop()
            .unwrap_or_default()
            .split(';')
            .filter_map(|s| s.split_once('='))
            .find(|(k, _)| k.trim().eq_ignore_ascii_case("charset"))
        {
            charset = charset.trim_matches('"');
            if let Ok(decoded) = iconv_native::decode_lossy(&buf, charset.trim()) {
                return Ok(decoded);
            }
        }
        Ok(String::from_utf8_lossy(&buf).into_owned())
    }

    async fn bytes(&mut self) -> nyquest_interface::Result<Vec<u8>> {
        let mut buf = vec![];
        while let Some(()) = self
            .handle
            .poll_bytes(|data| {
                if let Some(max_response_buffer_size) = self.max_response_buffer_size {
                    if buf.len() + data.len() > max_response_buffer_size as usize {
                        return Err(nyquest_interface::Error::ResponseTooLarge);
                    }
                }
                buf.extend_from_slice(data);
                Ok(())
            })
            .await?
        {}
        Ok(buf)
    }
}

impl nyquest_interface::r#async::AsyncClient for CurlMultiClient {
    type Response = CurlAsyncResponse;

    async fn request(
        &self,
        req: nyquest_interface::r#async::Request,
    ) -> nyquest_interface::Result<Self::Response> {
        let req = loop {
            // TODO: CURLOPT_SHARE
            let mut easy = Easy::new();
            // FIXME: properly concat base_url and url
            let url = concat_url(self.inner.options.base_url.as_deref(), &req.relative_uri);
            crate::request::populate_request(&url, &req, &self.inner.options, &mut easy)?;
            let req = self.inner.loop_manager.start_request(easy).await?;
            match req {
                r#loop::MaybeStartedRequest::Gone => {}
                r#loop::MaybeStartedRequest::Started(req) => break req,
            }
        };
        let mut res = req.wait_for_response().await?;
        res.max_response_buffer_size = self.inner.options.max_response_buffer_size;
        Ok(res)
    }
}

impl nyquest_interface::r#async::AsyncBackend for crate::CurlBackend {
    type AsyncClient = CurlMultiClient;

    async fn create_async_client(
        &self,
        options: nyquest_interface::client::ClientOptions,
    ) -> BuildClientResult<Self::AsyncClient> {
        Ok(CurlMultiClient {
            inner: Arc::new(CurlMultiClientInner {
                options,
                loop_manager: r#loop::LoopManager::new(),
            }),
        })
    }
}
