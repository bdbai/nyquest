use std::{
    future::poll_fn,
    io,
    pin::Pin,
    task::{ready, Context, Poll},
};

use bytes::Bytes;
use http::response::Parts;
use http_body::Body as _;
use nyquest_interface::Result as NyquestResult;

use crate::error::ReqwestBackendError;

#[derive(Debug)]
pub(crate) struct ReqwestResponse {
    parts: Parts,
    body: Pin<Box<reqwest::Body>>,
    buffer: Bytes,
    max_response_buffer_size: Option<u64>,
}

impl ReqwestResponse {
    pub fn new(response: reqwest::Response, max_response_buffer_size: Option<u64>) -> Self {
        let http_response: http::Response<reqwest::Body> = response.into();
        let (parts, body) = http_response.into_parts();

        Self {
            parts,
            body: Box::pin(body),
            buffer: Bytes::new(),
            max_response_buffer_size,
        }
    }

    pub fn status(&self) -> u16 {
        self.parts.status.as_u16()
    }

    pub fn content_length(&self) -> Option<u64> {
        self.body.size_hint().exact()
    }

    pub fn get_header(&self, header: &str) -> NyquestResult<Vec<String>> {
        Ok(self
            .parts
            .headers
            .get_all(header)
            .iter()
            .map(|v| v.to_str().unwrap_or_default().into())
            .collect::<Vec<_>>())
    }

    #[cfg(feature = "charset")]
    pub fn get_best_encoding(&self) -> &'static encoding_rs::Encoding {
        use encoding_rs::{Encoding, UTF_8};
        use http::header::CONTENT_TYPE;
        use mime::Mime;
        let content_type = self
            .parts
            .headers
            .get(CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.parse::<Mime>().ok());
        let encoding = content_type
            .as_ref()
            .and_then(|mime| mime.get_param("charset").map(|charset| charset.as_str()))
            .and_then(|charset| Encoding::for_label(charset.as_bytes()))
            .unwrap_or(UTF_8);

        encoding
    }

    pub async fn collect_all_bytes(&mut self) -> NyquestResult<Vec<u8>> {
        let mut bufs = vec![];
        let mut collected_size = 0;
        loop {
            let Some(frame) = self.receive_data_frame().await? else {
                break;
            };
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

    pub fn write_to(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let to_write = buf.len().min(self.buffer.len());
        if to_write > 0 {
            let src = self.buffer.split_to(to_write);
            buf[..to_write].copy_from_slice(&src);
            return Ok(to_write);
        }
        Ok(0)
    }

    fn poll_receive_data_frame(&mut self, cx: &mut Context<'_>) -> Poll<io::Result<Option<Bytes>>> {
        let frame = ready!(self.body.as_mut().poll_frame(cx));
        Poll::Ready(match frame {
            None => Ok(None),
            Some(Err(e)) => Err(io::Error::other(e)),
            Some(Ok(f)) => Ok(f.into_data().ok().filter(|d| !d.is_empty())),
        })
    }

    async fn receive_data_frame(&mut self) -> io::Result<Option<Bytes>> {
        poll_fn(|cx| self.poll_receive_data_frame(cx)).await
    }

    #[cfg(feature = "async")]
    pub fn poll_receive_data_frame_buffered(
        &mut self,
        cx: &mut Context<'_>,
    ) -> Poll<io::Result<usize>> {
        let buffer = ready!(self.poll_receive_data_frame(cx))?.unwrap_or_default();
        let len = buffer.len();
        self.buffer = buffer;
        Poll::Ready(Ok(len))
    }

    #[cfg(feature = "blocking")]
    pub async fn receive_data_frame_buffered(&mut self) -> io::Result<usize> {
        let buffer = self.receive_data_frame().await?.unwrap_or_default();
        let len = buffer.len();
        self.buffer = buffer;
        Ok(len)
    }
}
