//! Async WinHTTP response implementation.

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

        // Get raw handle without holding lock - callback may fire synchronously!
        let request_handle = self.ctx.get_request_raw();

        let result = unsafe { WinHttpQueryDataAvailable(request_handle, std::ptr::null_mut()) };
        if result == 0 {
            let err = crate::error::WinHttpError::from_last_error("WinHttpQueryDataAvailable");
            return Err(err.into());
        }
        Ok(())
    }

    /// Initiates reading data asynchronously.
    /// The data will be available in the data buffer after the READ_COMPLETE callback fires.
    #[cfg(feature = "async-stream")]
    fn start_read_data(&self, len: u32) -> NyquestResult<()> {
        use windows_sys::Win32::Networking::WinHttp::WinHttpReadData;

        let buffer = vec![0u8; len as usize];

        // Get raw handle without holding lock - callback may fire synchronously!
        let request_handle = self.ctx.get_request_raw();

        // Store the buffer in the context and get pointer to it
        let buffer_ptr = self.ctx.set_read_buffer(buffer);

        let result = unsafe {
            WinHttpReadData(
                request_handle,
                buffer_ptr as *mut std::ffi::c_void,
                len,
                std::ptr::null_mut(), // bytes_read - NULL for async mode
            )
        };
        if result == 0 {
            let err = crate::error::WinHttpError::from_last_error("WinHttpReadData");
            return Err(err.into());
        }
        Ok(())
    }

    /// Initiates reading data synchronously (for non-async-stream mode).
    #[cfg(not(feature = "async-stream"))]
    fn start_read_data(&self, len: u32) -> NyquestResult<Vec<u8>> {
        use windows_sys::Win32::Networking::WinHttp::WinHttpReadData;

        let mut buffer = vec![0u8; len as usize];
        let mut bytes_read: u32 = 0;

        // Get raw handle without holding lock - callback may fire synchronously!
        let request_handle = self.ctx.get_request_raw();

        let result = unsafe {
            WinHttpReadData(
                request_handle,
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

        // First, check if we have buffered data (from completed async read)
        if this.ctx.has_data() {
            let len = this.ctx.consume_data(buf);
            // Don't change state here - let the next poll handle querying for more
            return Poll::Ready(Ok(len));
        }

        let state = this.ctx.state();
        match state {
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
                this.ctx.set_waker(cx.waker().clone());

                // Attempt transition to QueryingData
                if this.ctx.transition_state_no_wake(
                    RequestState::HeadersReceived,
                    RequestState::QueryingData,
                ) {
                    if let Err(e) = this.start_query_data() {
                        return Poll::Ready(Err(std::io::Error::other(format!("{:?}", e))));
                    }
                } else {
                    // State changed while we were processing - re-poll
                    cx.waker().wake_by_ref();
                }
                Poll::Pending
            }
            RequestState::QueryingData => {
                // Query already in progress, just wait for callback
                this.ctx.set_waker(cx.waker().clone());
                // Double check state
                if this.ctx.state() != state {
                    cx.waker().wake_by_ref();
                }
                Poll::Pending
            }
            RequestState::DataAvailable => {
                let available = this.ctx.bytes_available();
                if available == 0 {
                    return Poll::Ready(Ok(0));
                }

                // Initiate async read
                this.ctx.set_waker(cx.waker().clone());

                // Attempt transition to Reading
                if this
                    .ctx
                    .transition_state_no_wake(RequestState::DataAvailable, RequestState::Reading)
                {
                    if let Err(e) = this.start_read_data(available) {
                        return Poll::Ready(Err(std::io::Error::other(format!("{:?}", e))));
                    }
                } else {
                    // State changed while we were processing - re-poll
                    cx.waker().wake_by_ref();
                }
                Poll::Pending
            }
            RequestState::Reading => {
                // Waiting for READ_COMPLETE callback
                this.ctx.set_waker(cx.waker().clone());
                // Double check state
                if this.ctx.state() != state {
                    cx.waker().wake_by_ref();
                }
                Poll::Pending
            }
            _ => {
                this.ctx.set_waker(cx.waker().clone());
                // Double check state
                if this.ctx.state() != state {
                    cx.waker().wake_by_ref();
                }
                Poll::Pending
            }
        }
    }
}

/// Future that waits for data to be available or completion.
/// Used in non-async-stream mode for the bytes() implementation.
#[cfg(not(feature = "async-stream"))]
struct WaitForData {
    ctx: Arc<RequestContext>,
}

#[cfg(not(feature = "async-stream"))]
impl std::future::Future for WaitForData {
    type Output = NyquestResult<()>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let ctx = &self.ctx;

        let state = ctx.state();
        match state {
            RequestState::Completed
            | RequestState::DataAvailable
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
                // Double check state
                if ctx.state() != state {
                    cx.waker().wake_by_ref();
                }
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

        // For async-stream, use AsyncRead to read all data
        #[cfg(feature = "async-stream")]
        {
            let mut result = Vec::new();
            let mut buf = [0u8; 8192];

            loop {
                // Create a future that polls read
                let mut pinned = Pin::new(&mut *this);
                let read_result = std::future::poll_fn(|cx| {
                    use nyquest_interface::r#async::futures_io::AsyncRead;
                    pinned.as_mut().poll_read(cx, &mut buf)
                })
                .await;

                match read_result {
                    Ok(0) => break, // EOF
                    Ok(n) => {
                        // Check buffer size limit
                        if result.len() as u64 + n as u64 > this.max_response_buffer_size {
                            return Err(nyquest_interface::Error::ResponseTooLarge);
                        }
                        result.extend_from_slice(&buf[..n]);
                    }
                    Err(e) => return Err(nyquest_interface::Error::Io(e)),
                }
            }
            Ok(result)
        }

        // For non-async-stream, use the blocking read approach
        #[cfg(not(feature = "async-stream"))]
        {
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
                        let available = this.ctx.bytes_available();

                        if available == 0 {
                            break;
                        }

                        // Check buffer size limit
                        if result.len() as u64 + available as u64 > this.max_response_buffer_size {
                            return Err(nyquest_interface::Error::ResponseTooLarge);
                        }

                        // Read the data synchronously
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
}
