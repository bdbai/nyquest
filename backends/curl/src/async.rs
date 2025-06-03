use std::io;
use std::task::{ready, Poll};
use std::{pin::Pin, sync::Arc, task::Context};

use curl::easy::Easy2;
use nyquest_interface::r#async::{futures_io, AsyncResponse};
use nyquest_interface::Error as NyquestError;

mod handler;
mod r#loop;
mod pause;

use crate::url::concat_url;

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

    async fn text(mut self: Pin<&mut Self>) -> nyquest_interface::Result<String> {
        let buf = self.as_mut().bytes().await?;
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

    async fn bytes(self: Pin<&mut Self>) -> nyquest_interface::Result<Vec<u8>> {
        let this = self.get_mut();
        let mut buf = vec![];
        while let Some(()) = this
            .handle
            .poll_bytes_async(|data| {
                if let Some(max_response_buffer_size) = this.max_response_buffer_size {
                    if buf.len() + data.len() > max_response_buffer_size as usize {
                        return Err(NyquestError::ResponseTooLarge);
                    }
                }
                buf.extend_from_slice(data);
                data.clear();
                Ok(())
            })
            .await?
        {}
        Ok(buf)
    }
}

impl futures_io::AsyncRead for CurlAsyncResponse {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut [u8],
    ) -> Poll<io::Result<usize>> {
        let this = self.get_mut();
        let poll_res = ready!(this.handle.poll_bytes(cx, |data| {
            let read_len = data.len().min(buf.len());
            buf[..read_len].copy_from_slice(&data[..read_len]);
            data.drain(..read_len);
            Ok(read_len)
        }));
        Poll::Ready(match poll_res {
            Ok(None) => Ok(0),
            Ok(Some(read_len)) => Ok(read_len),
            Err(NyquestError::Io(e)) => return Poll::Ready(Err(e)),
            Err(e) => unreachable!("Unexpected error: {}", e),
        })
    }
}

impl nyquest_interface::r#async::AsyncClient for CurlMultiClient {
    type Response = CurlAsyncResponse;

    async fn request(
        &self,
        req: nyquest_interface::r#async::Request,
    ) -> nyquest_interface::Result<Self::Response> {
        let req = {
            let mut easy = Easy2::new(handler::AsyncHandler::default());
            let raw_handle = easy.raw();
            easy.get_mut().pause = Some(pause::EasyPause::new(raw_handle));
            // FIXME: properly concat base_url and url
            let url = concat_url(self.inner.options.base_url.as_deref(), &req.relative_uri);
            crate::request::populate_request(
                &url,
                req,
                &self.inner.options,
                &mut easy,
                |_, _| unimplemented!(),
            )?;
            self.inner.loop_manager.start_request(easy).await?
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
    ) -> Result<Self::AsyncClient, NyquestError> {
        Ok(CurlMultiClient {
            inner: Arc::new(CurlMultiClientInner {
                loop_manager: r#loop::LoopManager::new(),
                options,
            }),
        })
    }
}
