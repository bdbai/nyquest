//! WinHTTP session handle wrapper.

use std::ptr::NonNull;

use windows_sys::Win32::Networking::WinHttp::*;

use crate::error::{Result, WinHttpError};

/// A wrapper around WinHTTP session handle (HINTERNET from WinHttpOpen).
///
/// The session handle is the top-level handle used to establish connections.
/// It can be shared across multiple requests and connections.
#[derive(Debug)]
pub(crate) struct SessionHandle {
    handle: NonNull<std::ffi::c_void>,
}

// WinHTTP session handles are thread-safe
unsafe impl Send for SessionHandle {}
unsafe impl Sync for SessionHandle {}

impl SessionHandle {
    /// Creates a new WinHTTP session.
    pub(crate) fn new(
        user_agent: Option<&str>,
        is_async: bool,
        use_default_proxy: bool,
    ) -> Result<Self> {
        let user_agent_wide: Vec<u16>;
        let user_agent_ptr = match user_agent {
            Some(ua) => {
                user_agent_wide = ua.encode_utf16().chain(std::iter::once(0)).collect();
                user_agent_wide.as_ptr()
            }
            None => std::ptr::null(),
        };

        let access_type = if use_default_proxy {
            WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY
        } else {
            WINHTTP_ACCESS_TYPE_NO_PROXY
        };
        let flags = if is_async { WINHTTP_FLAG_ASYNC } else { 0 };
        let handle = unsafe {
            WinHttpOpen(
                user_agent_ptr,
                access_type,
                std::ptr::null(),
                std::ptr::null(),
                flags,
            )
        };

        NonNull::new(handle)
            .map(|handle| Self { handle })
            .ok_or_else(|| WinHttpError::from_last_error("WinHttpOpen"))
    }

    /// Returns the raw handle.
    #[inline]
    pub(crate) fn as_raw(&self) -> *mut std::ffi::c_void {
        self.handle.as_ptr()
    }

    /// Sets the timeout values for the session.
    pub(crate) fn set_timeouts(
        &self,
        resolve_timeout: i32,
        connect_timeout: i32,
        send_timeout: i32,
        receive_timeout: i32,
    ) -> Result<()> {
        let result = unsafe {
            WinHttpSetTimeouts(
                self.as_raw(),
                resolve_timeout,
                connect_timeout,
                send_timeout,
                receive_timeout,
            )
        };
        if result == 0 {
            return Err(WinHttpError::from_last_error("WinHttpSetTimeouts"));
        }
        Ok(())
    }

    /// Sets an option on the session handle.
    pub(crate) unsafe fn set_option<T>(
        &self,
        option: u32,
        value: &T,
        error_context: &'static str,
    ) -> Result<()> {
        let result = unsafe {
            WinHttpSetOption(
                self.as_raw(),
                option,
                value as *const T as *const std::ffi::c_void,
                std::mem::size_of::<T>() as u32,
            )
        };
        if result == 0 {
            return Err(WinHttpError::from_last_error(error_context));
        }
        Ok(())
    }

    /// Disables automatic redirects.
    pub(crate) fn disable_redirects(&self) -> Result<()> {
        let policy: u32 = WINHTTP_OPTION_REDIRECT_POLICY_NEVER;
        unsafe {
            self.set_option(
                WINHTTP_OPTION_REDIRECT_POLICY,
                &policy,
                "WinHttpSetOption (disable_redirects)",
            )
        }
    }

    /// Enables automatic redirects.
    pub(crate) fn enable_redirects(&self) -> Result<()> {
        let policy: u32 = WINHTTP_OPTION_REDIRECT_POLICY_ALWAYS;
        unsafe {
            self.set_option(
                WINHTTP_OPTION_REDIRECT_POLICY,
                &policy,
                "WinHttpSetOption (enable_redirects)",
            )
        }
    }

    /// Sets the receive response timeout (time to wait for server to start sending response).
    pub(crate) fn set_receive_response_timeout(&self, timeout_ms: u32) -> Result<()> {
        unsafe {
            self.set_option(
                WINHTTP_OPTION_RECEIVE_RESPONSE_TIMEOUT,
                &timeout_ms,
                "WinHttpSetOption (set_receive_response_timeout)",
            )
        }
    }

    /// Sets the callback function for async operations.
    ///
    /// # Safety
    /// The callback must remain valid for the lifetime of all handles derived from this session.
    pub(crate) unsafe fn set_status_callback(
        &self,
        callback: WINHTTP_STATUS_CALLBACK,
        notification_flags: u32,
    ) -> Result<WINHTTP_STATUS_CALLBACK> {
        let prev = WinHttpSetStatusCallback(self.as_raw(), callback, notification_flags, 0);
        // Check WINHTTP_INVALID_STATUS_CALLBACK
        if unsafe { std::mem::transmute::<_, usize>(prev) } == usize::MAX {
            let error = windows_sys::Win32::Foundation::GetLastError();
            return Err(WinHttpError::from_code(error, "WinHttpSetStatusCallback"));
        }
        Ok(prev)
    }
}

impl Drop for SessionHandle {
    fn drop(&mut self) {
        unsafe {
            WinHttpCloseHandle(self.as_raw());
        }
    }
}
