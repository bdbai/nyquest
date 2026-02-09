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
    dw_status_information_length: u32,
) {
    // Context of 0 means the request context has been cleared (cleanup in progress)
    if dw_context == 0 {
        return;
    }

    // The context is a raw pointer to the RequestContext.
    // We need to be careful here - the RequestContext must be kept alive by the caller.
    // We just borrow it for the duration of this callback.
    let ctx_ptr = dw_context as *const RequestContext;
    let ctx = &*ctx_ptr;

    // Handle the callback based on status
    handle_callback(ctx, dw_internet_status, lpv_status_information, dw_status_information_length);
}

/// Handles a WinHTTP callback.
unsafe fn handle_callback(ctx: &RequestContext, status: u32, status_info: *mut c_void, status_info_len: u32) {
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
            handle_read_complete(ctx, status_info, status_info_len);
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
    // Query all data while holding the request lock
    let result = ctx.with_request(|request| {
        // Get status code
        let status = request.query_status_code()?;
        // Get content length
        let content_length = request.query_content_length();
        // Get headers
        let raw_headers = request.query_raw_headers()?;
        
        Ok::<_, WinHttpError>((status, content_length, raw_headers))
    });

    match result {
        Ok((status, content_length, raw_headers)) => {
            // Parse headers
            let mut headers = Vec::new();
            for line in raw_headers.lines() {
                if line.is_empty() || line.starts_with("HTTP/") {
                    continue;
                }
                if let Some((name, value)) = line.split_once(':') {
                    headers.push((name.trim().to_string(), value.trim().to_string()));
                }
            }
            
            // Now set all the metadata at once
            ctx.set_response_metadata(status as u32, content_length, headers);
        }
        Err(e) => {
            ctx.set_error(e);
            return;
        }
    }

    // Transition to HeadersReceived
    // We expect Previous state to be ReceivingResponse
    if !ctx.transition_state(RequestState::ReceivingResponse, RequestState::HeadersReceived) {
        // If transition failed, we might be in Error state or already Completed?
        // Logging would be good here, but we can't easily.
        // If we are in Error state, we should probably stay there.
    }
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
    ctx.set_bytes_available(bytes_available);

    if bytes_available == 0 {
        // No more data, request is complete
        // Transition from QueryingData to Completed.
        ctx.transition_state(RequestState::QueryingData, RequestState::Completed);
    } else {
        // Transition from QueryingData to DataAvailable
        ctx.transition_state(RequestState::QueryingData, RequestState::DataAvailable);
    }
}

/// Handles WINHTTP_CALLBACK_STATUS_READ_COMPLETE.
unsafe fn handle_read_complete(ctx: &RequestContext, _status_info: *mut c_void, status_info_len: u32) {
    // status_info is a pointer to the buffer (which we already have in context)
    // status_info_len contains the number of bytes read
    #[cfg(feature = "async-stream")]
    {
        let bytes_read = status_info_len as usize;
        
        if bytes_read > 0 {
            // Move data from read_buffer to data_buffer
            ctx.complete_read(bytes_read);
            // After read completes, query for more data
            // Use HeadersReceived to trigger poll_read query logic
            // Transition from Reading to HeadersReceived
            ctx.transition_state(RequestState::Reading, RequestState::HeadersReceived);
        } else {
            // No data read, we're done
            ctx.transition_state(RequestState::Reading, RequestState::Completed);
        }
    }
    
    #[cfg(not(feature = "async-stream"))]
    {
        // For non-async-stream, just notify that read is complete
        ctx.transition_state(RequestState::Reading, RequestState::QueryingData);
    }
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
