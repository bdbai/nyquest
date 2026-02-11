//! Async WinHTTP client implementation.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

use futures_channel::oneshot;
use nyquest_interface::client::ClientOptions;
use nyquest_interface::r#async::{AsyncBackend, AsyncClient, Request};
use nyquest_interface::{Error as NyquestError, Result as NyquestResult};

use super::callback::setup_session_callback;
use super::context::{RequestContext, RequestState};
use super::response::WinHttpAsyncResponse;
use super::threadpool::ThreadpoolTask;
use crate::error::{WinHttpError, WinHttpResultExt};
use crate::request::{
    create_request, method_to_cwstr, prepare_additional_headers, prepare_body, PreparedBody,
};
use crate::session::WinHttpSession;
use crate::stream::{DataOrStream, StreamWriter};
use crate::url::{concat_url, ParsedUrl};
use crate::WinHttpBackend;

#[cfg(feature = "async-stream")]
use nyquest_interface::r#async::BoxedStream;

/// Async WinHTTP client.
#[derive(Clone)]
pub struct WinHttpAsyncClient {
    session: Arc<WinHttpSession>,
}

impl WinHttpAsyncClient {
    pub(crate) async fn new(options: ClientOptions) -> NyquestResult<Self> {
        // Create async session
        let session = WinHttpSession::new(options, true).into_nyquest()?;

        // Set up the callback on the session
        setup_session_callback(&session.session).into_nyquest()?;

        Ok(Self { session })
    }

    /// Extracts content length from a BoxedStream if it's a sized stream.
    #[cfg(feature = "async-stream")]
    fn get_stream_content_length(stream: &BoxedStream) -> Option<u64> {
        match stream {
            BoxedStream::Sized { content_length, .. } => Some(*content_length),
            BoxedStream::Unsized { .. } => None,
        }
    }
}

impl AsyncClient for WinHttpAsyncClient {
    type Response = WinHttpAsyncResponse;

    fn request(&self, req: Request) -> impl Future<Output = NyquestResult<Self::Response>> + Send {
        let session = self.session.clone();
        async move {
            // Parse the URL
            let url = concat_url(session.base_cwurl.as_deref(), &req.relative_uri);

            let method = method_to_cwstr(&req.method);

            // Store additional headers before consuming body
            let additional_headers = req.additional_headers.clone();

            // Prepare headers and body before spawning to threadpool
            let mut headers_str = String::new();
            let prepared_body = prepare_body(req.body, &mut headers_str, |s| {
                #[cfg(feature = "async-stream")]
                {
                    Self::get_stream_content_length(s)
                }
                #[cfg(not(feature = "async-stream"))]
                {
                    let _ = s;
                    None
                }
            });
            headers_str.push_str(&prepare_additional_headers(
                &additional_headers,
                &session.options,
                &prepared_body,
            ));

            // Create the request context
            let ctx = RequestContext::new();

            // Extract body data and stream parts depending on the body type
            #[cfg(feature = "async-stream")]
            let (body_data, stream_parts) = match prepared_body {
                PreparedBody::None => (None, None),
                PreparedBody::Complete(data) => (Some(data), None),
                PreparedBody::Stream { stream_parts, .. } => (None, Some(stream_parts)),
            };
            #[cfg(not(feature = "async-stream"))]
            let (body_data, stream_parts): (
                Option<Vec<u8>>,
                Option<Vec<DataOrStream<()>>>,
            ) = match prepared_body {
                PreparedBody::None => (None, None),
                PreparedBody::Complete(data) => (Some(data), None),
            };

            // Determine content length for streaming
            #[cfg(feature = "async-stream")]
            let stream_content_length = stream_parts.as_ref().and_then(|parts| {
                // For single-stream uploads, use the stream's content length directly
                if parts.len() == 1 {
                    parts.iter().find_map(|p| {
                        if let DataOrStream::Stream(s) = p {
                            Self::get_stream_content_length(s)
                        } else {
                            None
                        }
                    })
                } else {
                    // For multipart, calculate total size from all parts
                    // This only works if ALL streams have known sizes
                    parts.iter().try_fold(0u64, |acc, part| match part {
                        DataOrStream::Data(d) => Some(acc + d.len() as u64),
                        DataOrStream::Stream(s) => {
                            Self::get_stream_content_length(s).map(|len| acc + len)
                        }
                    })
                }
            });
            #[cfg(not(feature = "async-stream"))]
            let stream_content_length: Option<u64> = None;

            // For unsized streams, add Transfer-Encoding: chunked header
            #[cfg(feature = "async-stream")]
            let is_chunked = stream_parts.as_ref().is_some_and(|parts| {
                parts.iter().any(|p| {
                    matches!(p, DataOrStream::Stream(s) if Self::get_stream_content_length(s).is_none())
                })
            });
            #[cfg(not(feature = "async-stream"))]
            let is_chunked = false;
            if is_chunked {
                headers_str.push_str("Transfer-Encoding: chunked\r\n");
            }

            // Create a oneshot channel for the initial setup result
            let (setup_tx, setup_rx) = oneshot::channel();

            // Clone data needed for the threadpool callback
            let max_response_buffer_size = session.options.max_response_buffer_size;
            let headers_owned = headers_str;

            // Store body data in context - it must remain valid until SENDREQUEST_COMPLETE
            ctx.set_body(body_data);

            // For streaming uploads, we need different setup
            #[cfg(feature = "async-stream")]
            let is_streaming = stream_parts.is_some();

            // Set the streaming flag on the context so the callback knows how to behave
            #[cfg(feature = "async-stream")]
            ctx.set_streaming_upload(is_streaming);

            // Submit the blocking connect/open/send to the threadpool
            let task = ThreadpoolTask::new(&ctx);
            task.submit(move |ctx| {
                let parsed_url = match ParsedUrl::parse(&url) {
                    Some(p) => p,
                    None => {
                        let _ = setup_tx.send(Err(NyquestError::InvalidUrl));
                        return;
                    }
                };

                #[cfg(feature = "async-stream")]
                let result = if is_streaming {
                    setup_and_send_streaming_request(
                        &session,
                        &ctx,
                        &parsed_url,
                        &method,
                        &headers_owned,
                        stream_content_length,
                    )
                } else {
                    setup_and_send_request(&session, &ctx, &parsed_url, &method, &headers_owned)
                };

                #[cfg(not(feature = "async-stream"))]
                let result =
                    setup_and_send_request(&session, &ctx, &parsed_url, &method, &headers_owned);

                let _ = setup_tx.send(result.into_nyquest());
            })?;

            // Wait for the setup to complete
            let () = setup_rx.await.map_err(|_| {
                nyquest_interface::Error::Io(std::io::Error::other("setup channel closed"))
            })??;

            // If streaming, poll the stream writer to send data
            #[cfg(feature = "async-stream")]
            if let Some(parts) = stream_parts {
                poll_stream_upload(ctx.clone(), parts, is_chunked).await?;
            }

            // Now wait for headers to be available
            let headers_future = WaitForHeaders { ctx: ctx.clone() };
            headers_future.await?;

            // Build the response
            let status = ctx.status_code() as u16;
            let content_length = ctx.content_length();
            let headers = ctx.headers();

            Ok(WinHttpAsyncResponse::new(
                ctx,
                status,
                content_length,
                headers,
                max_response_buffer_size,
            ))
        }
    }
}

/// Sets up and sends the request on the threadpool.
///
/// This function runs on the Win32 threadpool and performs the blocking
/// WinHTTP operations.
fn setup_and_send_request(
    session: &WinHttpSession,
    ctx: &RequestContext,
    parsed_url: &ParsedUrl,
    method_cwstr: &[u16],
    headers: &str,
) -> Result<(), WinHttpError> {
    // Create connection and request handles
    let (connection, request) = create_request(session, parsed_url, method_cwstr)?;
    // Add headers
    if !headers.is_empty() {
        request.add_headers(headers)?;
    }

    // Store the handles in the context
    ctx.set_handles(connection, request);

    // Get a raw pointer to the RequestContext to use as the callback context.
    // The RequestContext is kept alive by the Arc held by the caller.
    // WinHTTP will pass this pointer back to us in the callback.
    let ctx_ptr = ctx as *const RequestContext as usize;

    // Set the context on the request handle
    ctx.with_request(|request| unsafe { request.set_context(ctx_ptr) })?;

    // Update state to Sending
    ctx.set_state(RequestState::Sending);

    // Get the raw handle before calling send to avoid holding the lock during the async call.
    // WinHTTP can call the callback synchronously, which would cause a deadlock if we held the lock.
    let request_handle = ctx.get_request_raw();

    // Get the body pointer from the context - the body is stored there to ensure it lives
    // long enough for the async WinHTTP operation to complete.
    let (body_ptr, body_len) = ctx.get_body_ptr();

    // Send the request (async mode - returns immediately, but callback may fire synchronously)
    let result = unsafe {
        windows_sys::Win32::Networking::WinHttp::WinHttpSendRequest(
            request_handle,
            std::ptr::null(),
            0,
            body_ptr as *const std::ffi::c_void,
            body_len as u32,
            body_len as u32,
            ctx_ptr,
        )
    };

    if result == 0 {
        return Err(WinHttpError::from_last_error("WinHttpSendRequest"));
    }

    Ok(())
}

/// Sets up and sends the initial part of a streaming request.
///
/// This function runs on the Win32 threadpool. It sends the initial request with
/// no body data - the body will be written asynchronously via WinHttpWriteData.
#[cfg(feature = "async-stream")]
fn setup_and_send_streaming_request(
    session: &WinHttpSession,
    ctx: &RequestContext,
    parsed_url: &ParsedUrl,
    method_cwstr: &[u16],
    headers: &str,
    content_length: Option<u64>,
) -> Result<(), WinHttpError> {
    use windows_sys::Win32::Networking::WinHttp::WINHTTP_IGNORE_REQUEST_TOTAL_LENGTH;

    // Create connection and request handles
    let (connection, request) = create_request(session, parsed_url, method_cwstr)?;

    // Add headers
    if !headers.is_empty() {
        request.add_headers(headers)?;
    }

    // Store the handles in the context
    ctx.set_handles(connection, request);

    // Get a raw pointer to the RequestContext to use as the callback context.
    let ctx_ptr = ctx as *const RequestContext as usize;

    // Set the context on the request handle
    ctx.with_request(|request| unsafe { request.set_context(ctx_ptr) })?;

    // Update state to Sending
    ctx.set_state(RequestState::Sending);

    // Get the raw handle
    let request_handle = ctx.get_request_raw();

    // For streaming uploads, we send with no initial body and WINHTTP_IGNORE_REQUEST_TOTAL_LENGTH
    // if we don't know the content length (chunked transfer encoding).
    let total_length = content_length
        .map(|l| l as u32)
        .unwrap_or(WINHTTP_IGNORE_REQUEST_TOTAL_LENGTH);

    // Send the request with no body - we'll write data via WinHttpWriteData
    let result = unsafe {
        windows_sys::Win32::Networking::WinHttp::WinHttpSendRequest(
            request_handle,
            std::ptr::null(),
            0,
            std::ptr::null(),
            0,
            total_length,
            ctx_ptr,
        )
    };

    if result == 0 {
        return Err(WinHttpError::from_last_error(
            "WinHttpSendRequest (streaming)",
        ));
    }

    Ok(())
}

/// Polls the stream writer to send data chunks via WinHttpWriteData.
#[cfg(feature = "async-stream")]
async fn poll_stream_upload(
    ctx: Arc<RequestContext>,
    stream_parts: Vec<DataOrStream<BoxedStream>>,
    is_chunked: bool,
) -> NyquestResult<()> {
    // First, wait for SENDREQUEST_COMPLETE
    WaitForSendComplete { ctx: ctx.clone() }.await?;

    // Create stream writer
    let mut writer = StreamWriter::new(stream_parts, is_chunked);

    // Buffer for reading data
    const CHUNK_SIZE: usize = 65536;
    let mut buffer = vec![0u8; CHUNK_SIZE];

    loop {
        // Poll the stream writer to fill the buffer
        let poll_result = std::future::poll_fn(|cx| writer.poll_fill_buffer(cx, &mut buffer)).await;

        match poll_result {
            Ok(0) => {
                // No more data - done writing
                break;
            }
            Ok(n) => {
                // Got data, write it via WinHttpWriteData
                let data = buffer[..n].to_vec();
                write_data_async(&ctx, data).await?;
            }
            Err(e) => {
                return Err(nyquest_interface::Error::Io(e));
            }
        }
    }

    // If chunked, send the final chunk terminator
    if is_chunked {
        let final_chunk = writer.get_final_chunk().to_vec();
        write_data_async(&ctx, final_chunk).await?;
    }

    // Now initiate receiving the response
    initiate_receive_response(&ctx)?;

    Ok(())
}

/// Initiates WinHttpReceiveResponse after streaming upload completes.
#[cfg(feature = "async-stream")]
fn initiate_receive_response(ctx: &Arc<RequestContext>) -> NyquestResult<()> {
    use windows_sys::Win32::Networking::WinHttp::WinHttpReceiveResponse;

    // Transition to ReceivingResponse state
    ctx.set_state(RequestState::ReceivingResponse);

    // Get request handle
    let request_handle = ctx.get_request_raw();

    // Initiate WinHttpReceiveResponse
    let result = unsafe { WinHttpReceiveResponse(request_handle, std::ptr::null_mut()) };
    if result == 0 {
        return Err(WinHttpError::from_last_error("WinHttpReceiveResponse").into());
    }

    Ok(())
}

/// Writes data asynchronously via WinHttpWriteData and waits for completion.
#[cfg(feature = "async-stream")]
async fn write_data_async(ctx: &Arc<RequestContext>, data: Vec<u8>) -> NyquestResult<()> {
    if data.is_empty() {
        return Ok(());
    }

    // Store the data in the context so it remains valid during the async operation
    ctx.set_write_buffer(data);

    // Set state to Writing
    ctx.set_state(RequestState::Writing);

    // Get the buffer pointer and length
    let (ptr, len) = ctx.get_write_buffer_ptr();

    // Get request handle
    let request_handle = ctx.get_request_raw();

    // Call WinHttpWriteData
    let result = unsafe {
        windows_sys::Win32::Networking::WinHttp::WinHttpWriteData(
            request_handle,
            ptr as *const std::ffi::c_void,
            len as u32,
            std::ptr::null_mut(),
        )
    };

    if result == 0 {
        return Err(WinHttpError::from_last_error("WinHttpWriteData").into());
    }

    // Wait for WRITE_COMPLETE callback
    WaitForWriteComplete { ctx: ctx.clone() }.await
}

/// Future that waits for the send request to complete (SENDREQUEST_COMPLETE).
#[cfg(feature = "async-stream")]
struct WaitForSendComplete {
    ctx: Arc<RequestContext>,
}

#[cfg(feature = "async-stream")]
impl Future for WaitForSendComplete {
    type Output = NyquestResult<()>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let ctx = &self.ctx;

        let state = ctx.state();
        match state {
            RequestState::SendComplete | RequestState::Writing => Poll::Ready(Ok(())),
            RequestState::Error => {
                if let Some(err) = ctx.take_error() {
                    Poll::Ready(Err(err.into()))
                } else {
                    Poll::Ready(Err(nyquest_interface::Error::Io(std::io::Error::other(
                        "unknown error in send",
                    ))))
                }
            }
            _ => {
                ctx.set_waker(cx.waker().clone());
                // Double check state to avoid race condition (lost wakeup)
                if ctx.state() != state {
                    cx.waker().wake_by_ref();
                }
                Poll::Pending
            }
        }
    }
}

/// Future that waits for a write to complete (WRITE_COMPLETE).
#[cfg(feature = "async-stream")]
struct WaitForWriteComplete {
    ctx: Arc<RequestContext>,
}

#[cfg(feature = "async-stream")]
impl Future for WaitForWriteComplete {
    type Output = NyquestResult<()>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let ctx = &self.ctx;

        let state = ctx.state();
        match state {
            RequestState::WriteComplete | RequestState::SendComplete => Poll::Ready(Ok(())),
            RequestState::Error => {
                if let Some(err) = ctx.take_error() {
                    Poll::Ready(Err(err.into()))
                } else {
                    Poll::Ready(Err(nyquest_interface::Error::Io(std::io::Error::other(
                        "unknown error in write",
                    ))))
                }
            }
            _ => {
                ctx.set_waker(cx.waker().clone());
                // Double check state to avoid race condition (lost wakeup)
                if ctx.state() != state {
                    cx.waker().wake_by_ref();
                }
                Poll::Pending
            }
        }
    }
}

/// Future that waits for headers to be available.
struct WaitForHeaders {
    ctx: Arc<RequestContext>,
}

impl Future for WaitForHeaders {
    type Output = NyquestResult<()>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let ctx = &self.ctx;

        let state = ctx.state();
        match state {
            RequestState::HeadersReceived
            | RequestState::QueryingData
            | RequestState::DataAvailable
            | RequestState::Completed => Poll::Ready(Ok(())),
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
                // Double check state to avoid race condition (lost wakeup)
                if ctx.state() != state {
                    cx.waker().wake_by_ref();
                }
                Poll::Pending
            }
        }
    }
}

impl AsyncBackend for WinHttpBackend {
    type AsyncClient = WinHttpAsyncClient;

    async fn create_async_client(
        &self,
        options: ClientOptions,
    ) -> NyquestResult<Self::AsyncClient> {
        WinHttpAsyncClient::new(options).await
    }
}
