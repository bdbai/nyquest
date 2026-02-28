//! Win32 threadpool integration for async WinHTTP operations.

use windows_sys::Win32::Foundation::FALSE;
use windows_sys::Win32::System::Threading::{TrySubmitThreadpoolCallback, PTP_CALLBACK_INSTANCE};

use crate::error::{Result, WinHttpError};

/// Submits a callback to the Win32 threadpool.
///
/// This is used to run blocking WinHTTP operations (WinHttpConnect, WinHttpOpenRequest,
/// WinHttpSendRequest) on the threadpool instead of blocking the async runtime.
///
/// # Safety
/// The callback data must remain valid until the callback completes.
pub(crate) fn submit_callback<F>(callback: F) -> Result<()>
where
    F: FnOnce() + Send + 'static,
{
    // Box the closure and leak it - it will be freed in the callback wrapper
    let boxed: Box<F> = Box::new(callback);
    let raw = Box::into_raw(boxed);

    unsafe {
        let result = TrySubmitThreadpoolCallback(
            Some(threadpool_callback_wrapper::<F>),
            raw as *mut std::ffi::c_void,
            std::ptr::null_mut(),
        );

        if result == FALSE {
            // Callback was not submitted, we need to free the closure
            let _ = Box::from_raw(raw);
            return Err(WinHttpError::from_last_error("TrySubmitThreadpoolCallback"));
        }
    }

    Ok(())
}

/// Wrapper function for threadpool callbacks.
///
/// This is the actual callback that Windows calls. It extracts the Rust closure
/// and invokes it.
unsafe extern "system" fn threadpool_callback_wrapper<F: FnOnce() + Send + 'static>(
    _instance: PTP_CALLBACK_INSTANCE,
    context: *mut std::ffi::c_void,
) {
    if context.is_null() {
        return;
    }

    // Reconstruct the boxed closure
    let boxed: Box<F> = Box::from_raw(context as *mut _);

    // Catch panics to prevent unwinding across FFI boundary
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
        boxed();
    }));
}
