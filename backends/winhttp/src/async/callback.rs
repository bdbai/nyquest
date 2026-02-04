//! WinHTTP async callback implementation.

use std::ffi::c_void;

use windows_sys::Win32::Networking::WinHttp::*;

use super::context::{RequestContext, RequestState};
use crate::error::WinHttpError;

/// The WinHTTP status callback function.
///
/// This callback is invoked by WinHTTP when async operations complete or
/// when status changes occur.
///
/// # Safety
/// This function is called from WinHTTP and must handle all edge cases safely.
pub(crate) unsafe extern "system" fn winhttp_callback(
    _h_internet: *mut c_void,
    dw_context: usize,
    dw_internet_status: u32,
    lpv_status_information: *mut c_void,
    _dw_status_information_length: u32,
) {
    if dw_context == 0 {
        return;
    }

    // The context is a raw pointer to the RequestContext.
    // We need to be careful here - the RequestContext must be kept alive by the caller.
    // We just borrow it for the duration of this callback.
    let ctx_ptr = dw_context as *const RequestContext;
    let ctx = &*ctx_ptr;

    // Handle the callback based on status
    handle_callback(ctx, dw_internet_status, lpv_status_information);
}

/// Handles a WinHTTP callback.
unsafe fn handle_callback(ctx: &RequestContext, status: u32, status_info: *mut c_void) {
    match status {
        WINHTTP_CALLBACK_STATUS_SENDREQUEST_COMPLETE => {
            handle_send_complete(ctx);
        }
        WINHTTP_CALLBACK_STATUS_HEADERS_AVAILABLE => {
            handle_headers_available(ctx);
        }
        WINHTTP_CALLBACK_STATUS_DATA_AVAILABLE => {
            handle_data_available(ctx, status_info);
        }
        WINHTTP_CALLBACK_STATUS_READ_COMPLETE => {
            handle_read_complete(ctx, status_info);
        }
        WINHTTP_CALLBACK_STATUS_WRITE_COMPLETE => {
            handle_write_complete(ctx);
        }
        WINHTTP_CALLBACK_STATUS_REQUEST_ERROR => {
            handle_request_error(ctx, status_info);
        }
        WINHTTP_CALLBACK_STATUS_HANDLE_CLOSING => {
            // Handle is being closed, nothing special to do
        }
        _ => {
            // Other status codes we don't handle specially
        }
    }
}

/// Handles WINHTTP_CALLBACK_STATUS_SENDREQUEST_COMPLETE.
fn handle_send_complete(ctx: &RequestContext) {
    // The body data is no longer needed after the request is sent
    ctx.clear_body();

    // Transition from Sending to ReceivingResponse
    if ctx.transition_state(RequestState::Sending, RequestState::ReceivingResponse) {
        // Get the raw handle BEFORE calling the async function.
        // We must not hold the mutex while calling WinHttpReceiveResponse because
        // the callback for HEADERS_AVAILABLE may fire synchronously on the same thread.
        let h_request = ctx.get_request_raw();

        // Initiate WinHttpReceiveResponse - callback may be invoked synchronously!
        let result = unsafe { WinHttpReceiveResponse(h_request, std::ptr::null_mut()) };
        if result == 0 {
            let error = WinHttpError::from_last_error("WinHttpReceiveResponse");
            ctx.set_error(error);
        }
    }
}

/// Handles WINHTTP_CALLBACK_STATUS_HEADERS_AVAILABLE.
fn handle_headers_available(ctx: &RequestContext) {
    // Query status code and headers
    ctx.with_request(|request| {
        // Get status code
        match request.query_status_code() {
            Ok(status) => {
                ctx.status_code
                    .store(status as u32, std::sync::atomic::Ordering::Release);
            }
            Err(e) => {
                ctx.set_error(e);
                return;
            }
        }

        // Get content length
        *ctx.content_length.lock().unwrap() = request.query_content_length();

        // Get headers
        match request.query_raw_headers() {
            Ok(raw_headers) => {
                let mut headers = Vec::new();
                for line in raw_headers.lines() {
                    if line.is_empty() || line.starts_with("HTTP/") {
                        continue;
                    }
                    if let Some((name, value)) = line.split_once(':') {
                        headers.push((name.trim().to_string(), value.trim().to_string()));
                    }
                }
                *ctx.headers.lock().unwrap() = headers;
            }
            Err(e) => {
                ctx.set_error(e);
            }
        }
    });

    // Transition to HeadersReceived
    ctx.set_state(RequestState::HeadersReceived);
}

/// Handles WINHTTP_CALLBACK_STATUS_DATA_AVAILABLE.
unsafe fn handle_data_available(ctx: &RequestContext, status_info: *mut c_void) {
    if status_info.is_null() {
        ctx.set_error(WinHttpError::from_code(
            0,
            "null status_info in DATA_AVAILABLE",
        ));
        return;
    }

    let bytes_available = *(status_info as *const u32);
    ctx.bytes_available
        .store(bytes_available, std::sync::atomic::Ordering::Release);

    if bytes_available == 0 {
        // No more data, request is complete
        ctx.set_state(RequestState::Completed);
    } else {
        ctx.set_state(RequestState::DataAvailable);
    }
}

/// Handles WINHTTP_CALLBACK_STATUS_READ_COMPLETE.
fn handle_read_complete(ctx: &RequestContext, _status_info: *mut c_void) {
    // The status_info for READ_COMPLETE contains the buffer and bytes read
    // But we handle this differently - we check the actual read result

    // For streaming reads, we just wake the future to check the buffer
    ctx.set_state(RequestState::QueryingData);
}

/// Handles WINHTTP_CALLBACK_STATUS_WRITE_COMPLETE.
fn handle_write_complete(ctx: &RequestContext) {
    // Write completed, wake the future to continue
    ctx.wake();
}

/// Handles WINHTTP_CALLBACK_STATUS_REQUEST_ERROR.
unsafe fn handle_request_error(ctx: &RequestContext, status_info: *mut c_void) {
    if status_info.is_null() {
        ctx.set_error(WinHttpError::from_code(
            0,
            "null status_info in REQUEST_ERROR",
        ));
        return;
    }

    let result = &*(status_info as *const WINHTTP_ASYNC_RESULT);
    let error_code = result.dwError;

    ctx.set_error(WinHttpError::from_code(error_code, "async request error"));
}

/// Sets up the WinHTTP callback for a session.
///
/// Returns a boxed weak reference that must be kept alive and passed as context.
pub(crate) fn setup_session_callback(
    session: &crate::handle::SessionHandle,
) -> crate::error::Result<()> {
    unsafe {
        session.set_status_callback(
            Some(winhttp_callback),
            WINHTTP_CALLBACK_FLAG_ALL_NOTIFICATIONS,
        )?;
    }
    Ok(())
}
