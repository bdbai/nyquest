//! Async WinHTTP response implementation.

use std::future::poll_fn;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

use nyquest_interface::r#async::AsyncResponse;
use nyquest_interface::Result as NyquestResult;

use crate::error::WinHttpError;
use crate::handle::{ConnectionHandle, RequestHandle};
use crate::session::WinHttpSession;

use super::context::{RequestContext, RequestState};

/// Async WinHTTP response.
pub struct WinHttpAsyncResponse {
    ctx: Arc<RequestContext>,
    status: u16,
    content_length: Option<u64>,
    max_response_buffer_size: Option<u64>,
    _session: Arc<WinHttpSession>, // Keep session alive for the duration of the response
    _connection: ConnectionHandle,
    request: RequestHandle,
}

impl WinHttpAsyncResponse {
    pub(crate) fn new(
        ctx: Arc<RequestContext>,
        status: u16,
        content_length: Option<u64>,
        max_response_buffer_size: Option<u64>,
        session: Arc<WinHttpSession>,
        connection: ConnectionHandle,
        request: RequestHandle,
    ) -> Self {
        Self {
            ctx,
            status,
            content_length,
            max_response_buffer_size,
            _session: session,
            _connection: connection,
            request,
        }
    }
}

#[cfg(feature = "async-stream")]
impl nyquest_interface::r#async::futures_io::AsyncRead for WinHttpAsyncResponse {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut [u8],
    ) -> Poll<std::io::Result<usize>> {
        use std::task::ready;

        let res = ready!(self.poll_consume_data(cx, |data| {
            let len = data.len().min(buf.len());
            buf[..len].copy_from_slice(&data[..len]);
            len
        }));
        Poll::Ready(res.map_err(std::io::Error::other))
    }
}

impl WinHttpAsyncResponse {
    fn poll_consume_data(
        &mut self,
        cx: &mut Context<'_>,
        callback: impl FnOnce(&[u8]) -> usize,
    ) -> Poll<Result<usize, WinHttpError>> {
        let mut ctx = self.ctx.inner.lock().unwrap();
        if let Some(err) = ctx.error.take() {
            return Poll::Ready(Err(err));
        }
        match ctx.state {
            RequestState::DataAvailable => {
                let available = ctx.buffer_range.end;
                if available == 0 {
                    return Poll::Ready(Ok(0));
                }
                if ctx.buffer.len() < available {
                    ctx.buffer.resize(available, 0);
                }
                ctx.state = RequestState::Reading;
                ctx.buffer_range = 0..0;
                ctx.waker.clone_from(cx.waker());
                unsafe {
                    let ptr = ctx.buffer.as_mut_ptr() as _;
                    drop(ctx);
                    self.request.start_read_data(ptr, available)?;
                }
                Poll::Pending
            }
            RequestState::HeadersReceived | RequestState::Completed => {
                let available = &ctx.buffer[ctx.buffer_range.clone()];
                if ctx.state == RequestState::HeadersReceived && available.is_empty() {
                    ctx.state = RequestState::QueryingData;
                    ctx.buffer_range = 0..0;
                    ctx.waker.clone_from(cx.waker());
                    drop(ctx);
                    self.request.start_query_data_available()?;
                    return Poll::Pending;
                }

                let consumed = callback(available);
                ctx.buffer_range.start += consumed;

                Poll::Ready(Ok(consumed))
            }
            _ => {
                ctx.waker.clone_from(cx.waker());
                Poll::Pending
            }
        }
    }
}

impl AsyncResponse for WinHttpAsyncResponse {
    fn status(&self) -> u16 {
        self.status
    }

    fn content_length(&self) -> Option<u64> {
        self.content_length
    }

    fn get_header(&self, header: &str) -> NyquestResult<Vec<String>> {
        let headers = self.request.query_header(header)?;
        Ok(headers)
    }

    async fn text(mut self: Pin<&mut Self>) -> NyquestResult<String> {
        let bytes = self.as_mut().bytes().await?;

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
            if let Ok(decoded) = iconv_native::decode_lossy(&bytes, charset.trim()) {
                return Ok(decoded);
            }
        }

        Ok(String::from_utf8_lossy(&bytes).into_owned())
    }

    async fn bytes(self: Pin<&mut Self>) -> NyquestResult<Vec<u8>> {
        let this = self.get_mut();
        let max_size = this.max_response_buffer_size.unwrap_or(u64::MAX) as usize;
        let mut result = vec![];
        let mut read_len = usize::MAX;
        while read_len > 0 {
            let mut max_size_exceeded = false;
            read_len = poll_fn(|cx| {
                this.poll_consume_data(cx, |data| {
                    if data.len() + result.len() <= max_size {
                        result.extend_from_slice(data);
                    } else {
                        max_size_exceeded = true;
                    }
                    data.len()
                })
            })
            .await?;
            if max_size_exceeded {
                return Err(nyquest_interface::Error::ResponseTooLarge);
            }
        }
        Ok(result)
    }
}
