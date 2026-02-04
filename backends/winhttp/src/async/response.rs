//! Async WinHTTP response implementation.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

use nyquest_interface::r#async::AsyncResponse;
use nyquest_interface::Result as NyquestResult;

use super::context::{RequestContext, RequestState};

/// Async WinHTTP response.
pub struct WinHttpAsyncResponse {
    ctx: Arc<RequestContext>,
    status: u16,
    content_length: Option<u64>,
    headers: Vec<(String, String)>,
    max_response_buffer_size: u64,
}

impl WinHttpAsyncResponse {
    pub(crate) fn new(
        ctx: Arc<RequestContext>,
        status: u16,
        content_length: Option<u64>,
        headers: Vec<(String, String)>,
        max_response_buffer_size: u64,
    ) -> Self {
        Self {
            ctx,
            status,
            content_length,
            headers,
            max_response_buffer_size,
        }
    }

    fn detect_charset(&self) -> Option<String> {
        for (name, value) in &self.headers {
            if name.eq_ignore_ascii_case("content-type") {
                if let Some(charset_part) = value
                    .split(';')
                    .find(|s| s.trim().to_ascii_lowercase().starts_with("charset="))
                {
                    let charset = charset_part.trim().strip_prefix("charset=")?;
                    return Some(charset.trim_matches('"').to_string());
                }
            }
        }
        None
    }

    /// Initiates querying for available data.
    fn start_query_data(&self) -> NyquestResult<()> {
        use windows_sys::Win32::Networking::WinHttp::WinHttpQueryDataAvailable;

        self.ctx.with_request(|request| {
            let result =
                unsafe { WinHttpQueryDataAvailable(request.as_raw(), std::ptr::null_mut()) };
            if result == 0 {
                let err = crate::error::WinHttpError::from_last_error("WinHttpQueryDataAvailable");
                return Err(err.into());
            }
            Ok(())
        })
    }

    /// Initiates reading data.
    fn start_read_data(&self, len: u32) -> NyquestResult<Vec<u8>> {
        use windows_sys::Win32::Networking::WinHttp::WinHttpReadData;

        let mut buffer = vec![0u8; len as usize];
        let mut bytes_read: u32 = 0;

        self.ctx.with_request(|request| {
            let result = unsafe {
                WinHttpReadData(
                    request.as_raw(),
                    buffer.as_mut_ptr() as *mut std::ffi::c_void,
                    len,
                    &mut bytes_read,
                )
            };
            if result == 0 {
                let err = crate::error::WinHttpError::from_last_error("WinHttpReadData");
                return Err(err.into());
            }
            buffer.truncate(bytes_read as usize);
            Ok(buffer)
        })
    }
}

#[cfg(feature = "async-stream")]
impl nyquest_interface::r#async::futures_io::AsyncRead for WinHttpAsyncResponse {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut [u8],
    ) -> Poll<std::io::Result<usize>> {
        let this = self.get_mut();

        // First, check if we have buffered data
        if this.ctx.has_data() {
            let len = this.ctx.consume_data(buf);
            return Poll::Ready(Ok(len));
        }

        match this.ctx.state() {
            RequestState::Completed => {
                // No more data
                Poll::Ready(Ok(0))
            }
            RequestState::Error => {
                if let Some(err) = this.ctx.take_error() {
                    Poll::Ready(Err(std::io::Error::from(err)))
                } else {
                    Poll::Ready(Err(std::io::Error::other("unknown error")))
                }
            }
            RequestState::HeadersReceived => {
                // Start querying for data
                this.ctx.set_state(RequestState::QueryingData);
                if let Err(e) = this.start_query_data() {
                    return Poll::Ready(Err(std::io::Error::other(format!("{:?}", e))));
                }
                this.ctx.set_waker(cx.waker().clone());
                Poll::Pending
            }
            RequestState::QueryingData => {
                this.ctx.set_waker(cx.waker().clone());
                Poll::Pending
            }
            RequestState::DataAvailable => {
                let available = this
                    .ctx
                    .bytes_available
                    .load(std::sync::atomic::Ordering::Acquire);
                if available == 0 {
                    return Poll::Ready(Ok(0));
                }

                // Read the data synchronously since WinHTTP has already buffered it
                match this.start_read_data(available) {
                    Ok(data) => {
                        let len = data.len().min(buf.len());
                        buf[..len].copy_from_slice(&data[..len]);
                        if data.len() > len {
                            this.ctx.append_data(&data[len..]);
                        }
                        // Go back to querying for more data
                        this.ctx.set_state(RequestState::HeadersReceived);
                        Poll::Ready(Ok(len))
                    }
                    Err(e) => Poll::Ready(Err(std::io::Error::other(format!("{:?}", e)))),
                }
            }
            _ => {
                this.ctx.set_waker(cx.waker().clone());
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
        Ok(self
            .headers
            .iter()
            .filter(|(name, _)| name.eq_ignore_ascii_case(header))
            .map(|(_, value)| value.clone())
            .collect())
    }

    async fn text(mut self: Pin<&mut Self>) -> NyquestResult<String> {
        let bytes = self.as_mut().bytes().await?;

        // Try to detect charset from Content-Type header
        if let Some(charset) = self.detect_charset() {
            if charset.eq_ignore_ascii_case("utf-8") || charset.eq_ignore_ascii_case("us-ascii") {
                return Ok(String::from_utf8_lossy(&bytes).into_owned());
            }
        }

        Ok(String::from_utf8_lossy(&bytes).into_owned())
    }

    async fn bytes(self: Pin<&mut Self>) -> NyquestResult<Vec<u8>> {
        let this = self.get_mut();
        let mut result = Vec::new();

        loop {
            // Wait for data or completion
            let state_future = WaitForData {
                ctx: this.ctx.clone(),
            };
            state_future.await?;

            match this.ctx.state() {
                RequestState::Completed => break,
                RequestState::DataAvailable => {
                    let available = this
                        .ctx
                        .bytes_available
                        .load(std::sync::atomic::Ordering::Acquire);

                    if available == 0 {
                        break;
                    }

                    // Check buffer size limit
                    if result.len() as u64 + available as u64 > this.max_response_buffer_size {
                        return Err(nyquest_interface::Error::ResponseTooLarge);
                    }

                    // Read the data
                    let data = this.start_read_data(available)?;
                    result.extend_from_slice(&data);

                    // Continue querying for more data
                    this.ctx.set_state(RequestState::QueryingData);
                    this.start_query_data()?;
                }
                RequestState::HeadersReceived => {
                    // Start querying for data
                    this.ctx.set_state(RequestState::QueryingData);
                    this.start_query_data()?;
                }
                RequestState::Error => {
                    if let Some(err) = this.ctx.take_error() {
                        return Err(err.into());
                    }
                    return Err(nyquest_interface::Error::Io(std::io::Error::other(
                        "unknown error",
                    )));
                }
                _ => {
                    // Continue waiting
                }
            }
        }

        Ok(result)
    }
}

/// Future that waits for data to be available.
struct WaitForData {
    ctx: Arc<RequestContext>,
}

impl Future for WaitForData {
    type Output = NyquestResult<()>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let ctx = &self.ctx;

        match ctx.state() {
            RequestState::DataAvailable
            | RequestState::Completed
            | RequestState::HeadersReceived => Poll::Ready(Ok(())),
            RequestState::Error => {
                if let Some(err) = ctx.take_error() {
                    Poll::Ready(Err(err.into()))
                } else {
                    Poll::Ready(Err(nyquest_interface::Error::Io(std::io::Error::other(
                        "unknown error",
                    ))))
                }
            }
            _ => {
                ctx.set_waker(cx.waker().clone());
                Poll::Pending
            }
        }
    }
}
