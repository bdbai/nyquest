//! Async WinHTTP client implementation.

use std::future::Future;
use std::sync::Arc;

use futures_channel::oneshot;
use nyquest_interface::client::ClientOptions;
use nyquest_interface::r#async::{AsyncBackend, AsyncClient, Request};
use nyquest_interface::{Error as NyquestError, Result as NyquestResult};

use super::callback::setup_session_callback;
use super::context::{RequestContext, RequestState};
use super::response::WinHttpAsyncResponse;
use super::threadpool::ThreadpoolTask;
use crate::error::{WinHttpError, WinHttpResultExt};
use crate::handle::{ConnectionHandle, RequestHandle};
use crate::r#async::state_fut::wait_for_state;
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
            let mut prepared_body =
                prepare_body(req.body, &mut headers_str, get_stream_content_length);
            headers_str.push_str(&prepare_additional_headers(
                &additional_headers,
                &session.options,
                &prepared_body,
            ));

            // Create the request context
            let ctx = RequestContext::new();

            let body_len = prepared_body.body_len(get_stream_content_length);
            if body_len.is_none() {
                headers_str.push_str("Transfer-Encoding: chunked\r\n");
            }

            // Create a oneshot channel for the initial setup result
            let (setup_tx, setup_rx) = oneshot::channel();

            // Clone data needed for the threadpool callback
            let max_response_buffer_size = session.options.max_response_buffer_size;
            let headers_owned = headers_str;

            let is_stream = matches!(prepared_body, PreparedBody::Stream { .. });
            // Store body data in context - it must remain valid until SENDREQUEST_COMPLETE
            ctx.set_body(prepared_body.take_body());

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

                let result = if is_stream {
                    #[cfg(feature = "async-stream")]
                    {
                        setup_and_send_streaming_request(
                            &session,
                            &ctx,
                            &parsed_url,
                            &method,
                            &headers_owned,
                            body_len,
                        )
                    }
                    #[cfg(not(feature = "async-stream"))]
                    unreachable!("streaming requires async-stream feature")
                } else {
                    setup_and_send_request(&session, &ctx, &parsed_url, &method, &headers_owned)
                };

                let _ = setup_tx.send(result.into_nyquest());
            })?;

            // Wait for the setup to complete
            let (connection, request) = setup_rx.await.map_err(|_| {
                nyquest_interface::Error::Io(std::io::Error::other("setup channel closed"))
            })??;

            wait_for_state(&*ctx, RequestState::HeadersSent).await?;

            // If streaming, poll the stream writer to send data
            #[cfg(feature = "async-stream")]
            if let PreparedBody::Stream { stream_parts, .. } = prepared_body {
                poll_stream_upload(&ctx, &request, stream_parts, body_len.is_none()).await?;
            }

            request.receive_response().into_nyquest()?;

            // Now wait for headers to be available
            wait_for_state(&*ctx, RequestState::HeadersReceived).await?;

            // Build the response
            let status = request.query_status_code()?;
            let content_length = request.query_content_length();

            Ok(WinHttpAsyncResponse::new(
                ctx,
                status,
                content_length,
                max_response_buffer_size,
                connection,
                request,
            ))
        }
    }
}

/// Extracts content length from a BoxedStream if it's a sized stream.
#[cfg(feature = "async-stream")]
fn get_stream_content_length(stream: &BoxedStream) -> Option<u64> {
    match stream {
        BoxedStream::Sized { content_length, .. } => Some(*content_length),
        BoxedStream::Unsized { .. } => None,
    }
}

#[cfg(not(feature = "async-stream"))]
fn get_stream_content_length(_stream: &impl Sized) -> Option<u64> {
    None
}

/// Sets up and sends the request on the threadpool.
///
/// This function runs on the Win32 threadpool and performs the blocking
/// WinHTTP operations.
fn setup_and_send_request(
    session: &WinHttpSession,
    ctx: &Arc<RequestContext>,
    parsed_url: &ParsedUrl,
    method_cwstr: &[u16],
    headers: &str,
) -> Result<(ConnectionHandle, RequestHandle), WinHttpError> {
    // Create connection and request handles
    let (connection, request) = create_request(session, parsed_url, method_cwstr)?;
    // Add headers
    if !headers.is_empty() {
        request.add_headers(headers)?;
    }

    let ctx_ptr = Arc::downgrade(ctx).into_raw() as usize;

    // Get the body pointer from the context - the body is stored there to ensure it lives
    // long enough for the async WinHTTP operation to complete.
    let (body_ptr, body_len) = ctx.get_body_ptr();

    // Send the request (async mode - returns immediately, but callback may fire synchronously)
    let result = unsafe {
        windows_sys::Win32::Networking::WinHttp::WinHttpSendRequest(
            request.as_raw(),
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

    Ok((connection, request))
}

/// Sets up and sends the initial part of a streaming request.
///
/// This function runs on the Win32 threadpool. It sends the initial request with
/// no body data - the body will be written asynchronously via WinHttpWriteData.
#[cfg(feature = "async-stream")]
fn setup_and_send_streaming_request(
    session: &WinHttpSession,
    ctx: &Arc<RequestContext>,
    parsed_url: &ParsedUrl,
    method_cwstr: &[u16],
    headers: &str,
    content_length: Option<u64>,
) -> Result<(ConnectionHandle, RequestHandle), WinHttpError> {
    use windows_sys::Win32::Networking::WinHttp::WINHTTP_IGNORE_REQUEST_TOTAL_LENGTH;

    // Create connection and request handles
    let (connection, request) = create_request(session, parsed_url, method_cwstr)?;

    // Add headers
    if !headers.is_empty() {
        request.add_headers(headers)?;
    }

    // Get a raw pointer to the RequestContext to use as the callback context.
    let ctx_ptr = Arc::downgrade(ctx).into_raw() as usize;

    // For streaming uploads, we send with no initial body and WINHTTP_IGNORE_REQUEST_TOTAL_LENGTH
    // if we don't know the content length (chunked transfer encoding).
    let total_length = content_length
        .map(|l| l as u32)
        .unwrap_or(WINHTTP_IGNORE_REQUEST_TOTAL_LENGTH);

    // Send the request with no body - we'll write data via WinHttpWriteData
    let result = unsafe {
        windows_sys::Win32::Networking::WinHttp::WinHttpSendRequest(
            request.as_raw(),
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

    Ok((connection, request))
}

/// Polls the stream writer to send data chunks via WinHttpWriteData.
#[cfg(feature = "async-stream")]
async fn poll_stream_upload(
    ctx: &RequestContext,
    request: &RequestHandle,
    stream_parts: Vec<DataOrStream<BoxedStream>>,
    is_chunked: bool,
) -> NyquestResult<()> {
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
                write_data_async(ctx, request, data).await?;
            }
            Err(e) => {
                return Err(nyquest_interface::Error::Io(e));
            }
        }
    }

    // If chunked, send the final chunk terminator
    if is_chunked {
        let final_chunk = writer.get_final_chunk().to_vec();
        write_data_async(ctx, request, final_chunk).await?;
    }
    Ok(())
}

/// Writes data asynchronously via WinHttpWriteData and waits for completion.
#[cfg(feature = "async-stream")]
async fn write_data_async(
    ctx: &RequestContext,
    request: &RequestHandle,
    data: Vec<u8>,
) -> NyquestResult<()> {
    if data.is_empty() {
        return Ok(());
    }

    // Store the data in the context so it remains valid during the async operation
    ctx.set_write_buffer(data);

    // Set state to Writing
    ctx.set_state(RequestState::HeadersSent);

    // Get the buffer pointer and length
    let (ptr, len) = ctx.get_write_buffer_ptr();

    // Call WinHttpWriteData
    let result = unsafe {
        windows_sys::Win32::Networking::WinHttp::WinHttpWriteData(
            request.as_raw(),
            ptr as *const std::ffi::c_void,
            len as u32,
            std::ptr::null_mut(),
        )
    };

    if result == 0 {
        return Err(WinHttpError::from_last_error("WinHttpWriteData").into());
    }

    // Wait for WRITE_COMPLETE callback
    wait_for_state(ctx, RequestState::WriteComplete).await?;
    Ok(())
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
