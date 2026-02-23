//! Async WinHTTP response implementation.

use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

use nyquest_interface::r#async::AsyncResponse;
use nyquest_interface::Result as NyquestResult;

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

    /// Initiates querying for available data.
    fn start_query_data(&self) -> NyquestResult<()> {
        use windows_sys::Win32::Networking::WinHttp::WinHttpQueryDataAvailable;

        let result =
            unsafe { WinHttpQueryDataAvailable(self.request.as_raw(), std::ptr::null_mut()) };
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

        // Store the buffer in the context and get pointer to it
        let buffer_ptr = self.ctx.set_read_buffer(buffer);

        let result = unsafe {
            WinHttpReadData(
                self.request.as_raw(),
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

        let result = unsafe {
            WinHttpReadData(
                self.request.as_raw(),
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
                this.ctx.set_waker(cx.waker());

                // Attempt transition to QueryingData
                this.ctx
                    .transition_state_no_wake(RequestState::QueryingData);
                if let Err(e) = this.start_query_data() {
                    return Poll::Ready(Err(std::io::Error::other(format!("{:?}", e))));
                }

                Poll::Pending
            }
            RequestState::DataAvailable => {
                let available = this.ctx.bytes_available();
                if available == 0 {
                    return Poll::Ready(Ok(0));
                }

                // Initiate async read
                this.ctx.set_waker(cx.waker());

                // Attempt transition to Reading
                this.ctx.transition_state_no_wake(RequestState::Reading);
                if let Err(e) = this.start_read_data(available) {
                    return Poll::Ready(Err(std::io::Error::other(format!("{:?}", e))));
                }
                Poll::Pending
            }
            _ => {
                this.ctx.set_waker(cx.waker());
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
                ctx.set_waker(&cx.waker());
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

                match (read_result, this.max_response_buffer_size) {
                    (Ok(0), _) => break, // EOF
                    (Ok(n), Some(max_size)) => {
                        // Check buffer size limit
                        if result.len() as u64 + n as u64 > max_size {
                            return Err(nyquest_interface::Error::ResponseTooLarge);
                        }
                        result.extend_from_slice(&buf[..n]);
                    }
                    (Ok(n), None) => {
                        result.extend_from_slice(&buf[..n]);
                    }
                    (Err(e), _) => return Err(nyquest_interface::Error::Io(e)),
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
                        if let Some(max_size) = this.max_response_buffer_size {
                            if result.len() as u64 + available as u64 > max_size {
                                return Err(nyquest_interface::Error::ResponseTooLarge);
                            }
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
