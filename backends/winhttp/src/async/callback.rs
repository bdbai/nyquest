//! WinHTTP async callback implementation.

use std::{ffi::c_void, sync::Arc};

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

    let ctx = if dw_internet_status == WINHTTP_CALLBACK_STATUS_HANDLE_CLOSING {
        let ctx = unsafe { Arc::<RequestContext>::from_raw(dw_context as _) };
        drop(ctx);
        return;
    } else {
        unsafe { &*(dw_context as *const RequestContext) }
    };

    // Handle the callback based on status
    handle_callback(
        ctx,
        dw_internet_status,
        lpv_status_information,
        dw_status_information_length,
    );
}

/// Handles a WinHTTP callback.
unsafe fn handle_callback(
    ctx: &RequestContext,
    status: u32,
    status_info: *mut c_void,
    status_info_len: u32,
) {
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
            handle_write_complete(ctx, status_info);
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
    ctx.set_send_complete();
}

/// Handles WINHTTP_CALLBACK_STATUS_HEADERS_AVAILABLE.
fn handle_headers_available(ctx: &RequestContext) {
    ctx.transition_state(RequestState::HeadersReceived);
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
        ctx.transition_state(RequestState::Completed);
    } else {
        // Transition from QueryingData to DataAvailable
        ctx.transition_state(RequestState::DataAvailable);
    }
}

/// Handles WINHTTP_CALLBACK_STATUS_READ_COMPLETE.
unsafe fn handle_read_complete(
    ctx: &RequestContext,
    status_info: *mut c_void,
    status_info_len: u32,
) {
    let bytes_read = status_info_len as usize;
    ctx.set_read_complete(status_info as _, bytes_read);
}

/// Handles WINHTTP_CALLBACK_STATUS_WRITE_COMPLETE.
unsafe fn handle_write_complete(ctx: &RequestContext, status_info: *mut c_void) {
    // Transition from Writing to WriteComplete
    let bytes_written = if status_info.is_null() {
        0
    } else {
        unsafe { *(status_info as *const u32) as usize }
    };
    ctx.set_write_complete(bytes_written);
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
