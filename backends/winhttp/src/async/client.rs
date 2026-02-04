//! Async WinHTTP client implementation.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

use futures_channel::oneshot;
use nyquest_interface::client::ClientOptions;
use nyquest_interface::r#async::{AsyncBackend, AsyncClient, Request};
use nyquest_interface::Result as NyquestResult;

use super::callback::setup_session_callback;
use super::context::{RequestContext, RequestState};
use super::response::WinHttpAsyncResponse;
use super::threadpool::ThreadpoolTask;
use crate::error::{WinHttpError, WinHttpResultExt};
use crate::request::{
    create_request, method_to_str, prepare_additional_headers, prepare_body, PreparedBody,
};
use crate::session::WinHttpSession;
use crate::url::{concat_url, ParsedUrl};
use crate::WinHttpBackend;

/// Async WinHTTP client.
#[derive(Clone)]
pub struct WinHttpAsyncClient {
    session: Arc<WinHttpSession>,
}

impl WinHttpAsyncClient {
    pub(crate) async fn new(options: ClientOptions) -> NyquestResult<Self> {
        // Create async session
        let session = WinHttpSession::new_async(options).into_nyquest()?;

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
            let url = concat_url(session.options.base_url.as_deref(), &req.relative_uri);
            let parsed_url = ParsedUrl::parse(&url).ok_or(nyquest_interface::Error::InvalidUrl)?;

            let method = method_to_str(&req.method);

            // Store additional headers before consuming body
            let additional_headers = req.additional_headers.clone();

            // Prepare headers and body before spawning to threadpool
            let mut headers_str = String::new();
            let (prepared_body, _stream) = prepare_body(req.body, &mut headers_str);
            headers_str.push_str(&prepare_additional_headers(
                &additional_headers,
                &session.options,
                &prepared_body,
            ));

            // Create the request context
            let ctx = RequestContext::new();

            // Create a oneshot channel for the initial setup result
            let (setup_tx, setup_rx) = oneshot::channel::<Result<(), WinHttpError>>();

            // Clone data needed for the threadpool callback
            let session_clone = session.clone();
            let parsed_url_owned = parsed_url;
            let method_owned = method.to_string();
            let headers_owned = headers_str;
            let body_data = match &prepared_body {
                PreparedBody::None => None,
                PreparedBody::Complete(data) => Some(data.clone()),
                #[cfg(feature = "async-stream")]
                PreparedBody::Stream { .. } => None, // Handled separately
            };

            // Store body data in context - it must remain valid until SENDREQUEST_COMPLETE
            ctx.set_body(body_data);

            // Submit the blocking connect/open/send to the threadpool
            let task = ThreadpoolTask::new(&ctx);
            task.submit(move |ctx| {
                let result = setup_and_send_request(
                    &session_clone,
                    &ctx,
                    &parsed_url_owned,
                    &method_owned,
                    &headers_owned,
                );
                let _ = setup_tx.send(result);
            })
            .into_nyquest()?;

            // Wait for the setup to complete
            let setup_result = setup_rx.await.map_err(|_| {
                nyquest_interface::Error::Io(std::io::Error::other("setup channel closed"))
            })?;
            setup_result.into_nyquest()?;

            // Now wait for headers to be available
            let headers_future = WaitForHeaders { ctx: ctx.clone() };
            headers_future.await?;

            // Build the response
            let status = ctx.status_code.load(std::sync::atomic::Ordering::Acquire) as u16;
            let content_length = *ctx.content_length.lock().unwrap();
            let headers = ctx.headers.lock().unwrap().clone();

            Ok(WinHttpAsyncResponse::new(
                ctx,
                status,
                content_length,
                headers,
                session.max_response_buffer_size(),
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
    method: &str,
    headers: &str,
) -> Result<(), WinHttpError> {
    // Create connection and request handles
    let (connection, request) = create_request(session, parsed_url, method)?;

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

/// Future that waits for headers to be available.
struct WaitForHeaders {
    ctx: Arc<RequestContext>,
}

impl Future for WaitForHeaders {
    type Output = NyquestResult<()>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let ctx = &self.ctx;

        match ctx.state() {
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
