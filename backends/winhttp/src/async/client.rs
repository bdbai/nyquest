//! Async WinHTTP client implementation.

use std::future::Future;
#[cfg(feature = "async-stream")]
use std::ops::Range;
use std::sync::Arc;

use futures_channel::oneshot;
use nyquest_interface::client::ClientOptions;
use nyquest_interface::r#async::{AsyncBackend, AsyncClient, Request};
use nyquest_interface::{Error as NyquestError, Result as NyquestResult};

use super::callback::setup_session_callback;
use super::context::{RequestContext, RequestState};
use super::response::WinHttpAsyncResponse;
use crate::error::{WinHttpError, WinHttpResultExt};
use crate::handle::RequestHandle;
use crate::r#async::state_fut::wait_for_state;
use crate::r#async::threadpool::submit_callback;
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
            // Prepare headers and body before spawning to threadpool
            let mut prepared_body;

            // Create the request context
            let ctx = RequestContext::new();

            let body_len;
            let (setup_tx, setup_rx) = oneshot::channel();
            submit_callback({
                let url = concat_url(session.base_cwurl.as_deref(), &req.relative_uri)?;
                let method = method_to_cwstr(&req.method);
                prepared_body = prepare_body(req.body, get_stream_content_length);
                let headers_str = prepare_additional_headers(
                    &req.additional_headers,
                    &session.options,
                    &prepared_body,
                );

                body_len = prepared_body.body_len();
                let is_stream = matches!(prepared_body, PreparedBody::Stream { .. });
                // Store body data in context - it must remain valid until SENDREQUEST_COMPLETE
                ctx.set_body(prepared_body.take_body().unwrap_or_default());

                let ctx = Arc::downgrade(&ctx);
                let session = session.clone();
                move || {
                    let parsed_url = match ParsedUrl::parse(&url) {
                        Some(p) => p,
                        None => {
                            let _ = setup_tx.send(Err(NyquestError::InvalidUrl));
                            return;
                        }
                    };

                    let (connection, request) = match create_request(&session, &parsed_url, &method)
                    {
                        Ok(handles) => handles,
                        Err(e) => {
                            let _ = setup_tx.send(Err(e.into()));
                            return;
                        }
                    };
                    drop(session);
                    let Some(ctx) = ctx.upgrade() else {
                        return;
                    };
                    let result = if headers_str.is_empty() {
                        Ok(())
                    } else {
                        request.add_headers(&headers_str)
                    };
                    let result = result.and_then(|()| {
                        let context = Arc::into_raw(ctx.clone()) as usize;
                        let res = match (is_stream, body_len) {
                            (true, Some(len)) => request.send_with_total_length(len, context),
                            (true, None) => request.send_chunked(context),
                            (false, _) => {
                                let (body_ptr, body_len) = ctx.get_body_ptr();
                                unsafe { request.send(body_ptr, body_len, context) }
                            }
                        };
                        if res.is_err() {
                            let _ = unsafe { Arc::from_raw(context as *const RequestContext) };
                        }
                        res
                    });

                    let _ = setup_tx.send(result.map(|()| (connection, request)).into_nyquest());
                }
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
                session.options.max_response_buffer_size,
                session.clone(),
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

    while !writer.is_finished() {
        let (buf, range) = writer
            .take_buffer(|stream, buf, cx| {
                use nyquest_interface::r#async::futures_io::AsyncRead as _;
                use std::pin::Pin;

                Pin::new(stream).poll_read(cx, buf)
            })
            .await?;
        let buf = write_all_data_async(ctx, request, buf, range).await?;
        writer.advance(buf);
    }
    Ok(())
}

/// Writes data asynchronously via WinHttpWriteData and waits for completion.
#[cfg(feature = "async-stream")]
async fn write_all_data_async(
    ctx: &RequestContext,
    request: &RequestHandle,
    data: Vec<u8>,
    mut range: Range<usize>,
) -> NyquestResult<Vec<u8>> {
    if data.is_empty() {
        return Ok(Vec::new());
    }

    // Store the data in the context so it remains valid during the async operation
    ctx.set_write_buffer(data.into());
    while !range.is_empty() {
        let ptr = ctx.prepare_for_writing();

        let result = unsafe {
            windows_sys::Win32::Networking::WinHttp::WinHttpWriteData(
                request.as_raw(),
                ptr.add(range.start) as *const std::ffi::c_void,
                range.len() as u32,
                std::ptr::null_mut(),
            )
        };

        if result == 0 {
            return Err(WinHttpError::from_last_error("WinHttpWriteData").into());
        }

        // Wait for WRITE_COMPLETE callback
        let res = wait_for_state(ctx, RequestState::WriteComplete).await?;
        range.start += res.bytes_transferred;
    }
    Ok(ctx.take_write_buffer().into_owned())
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
