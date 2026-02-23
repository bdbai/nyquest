//! WinHTTP request handle wrapper.

use std::{mem::MaybeUninit, ptr::NonNull};

use windows_sys::Win32::Networking::WinHttp::*;

use super::connection::ConnectionHandle;
use crate::error::{Result, WinHttpError};

/// A wrapper around WinHTTP request handle (HINTERNET from WinHttpOpenRequest).
///
/// The request handle represents a single HTTP request.
#[derive(Debug)]
pub(crate) struct RequestHandle {
    handle: NonNull<std::ffi::c_void>,
}

// WinHTTP request handles are thread-safe
unsafe impl Send for RequestHandle {}
unsafe impl Sync for RequestHandle {}

impl RequestHandle {
    /// Creates a new HTTP request.
    pub(crate) fn open(
        connection: &ConnectionHandle,
        method_cwstr: &[u16],
        path: &[u16],
        is_secure: bool,
    ) -> Result<Self> {
        if !method_cwstr.ends_with(&[0]) {
            panic!("method_cwstr must be null-terminated");
        }
        let path_wide: Vec<u16> = path.iter().cloned().chain(std::iter::once(0)).collect();

        let flags = if is_secure { WINHTTP_FLAG_SECURE } else { 0 };

        let handle = unsafe {
            WinHttpOpenRequest(
                connection.as_raw(),
                method_cwstr.as_ptr(),
                path_wide.as_ptr(),
                std::ptr::null(), // Use default HTTP version
                std::ptr::null(), // No referrer
                std::ptr::null(), // Accept all types
                flags,
            )
        };

        NonNull::new(handle)
            .map(|handle| Self { handle })
            .ok_or_else(|| WinHttpError::from_last_error("WinHttpOpenRequest"))
    }

    /// Returns the raw handle.
    #[inline]
    pub(crate) fn as_raw(&self) -> *mut std::ffi::c_void {
        self.handle.as_ptr()
    }

    /// Adds HTTP headers to the request.
    pub(crate) fn add_headers(&self, headers: &str) -> Result<()> {
        let headers_wide: Vec<u16> = headers.encode_utf16().chain(std::iter::once(0)).collect();

        let result = unsafe {
            WinHttpAddRequestHeaders(
                self.as_raw(),
                headers_wide.as_ptr(),
                headers_wide.len() as u32 - 1, // Exclude null terminator
                WINHTTP_ADDREQ_FLAG_ADD | WINHTTP_ADDREQ_FLAG_REPLACE,
            )
        };

        if result == 0 {
            return Err(WinHttpError::from_last_error("WinHttpAddRequestHeaders"));
        }
        Ok(())
    }

    /// Sends the HTTP request with optional body data.
    ///
    /// # Safety
    /// The caller must ensure that the body data remains valid until the
    /// request is sent.
    pub(crate) unsafe fn send(
        &self,
        mut body_ptr: *const u8,
        body_len: usize,
        context: usize,
    ) -> Result<()> {
        let body_len = body_len as u32;
        if body_len == 0 {
            body_ptr = std::ptr::null();
        }

        let result = unsafe {
            WinHttpSendRequest(
                self.as_raw(),
                std::ptr::null(), // No additional headers
                0,
                body_ptr as _,
                body_len,
                body_len,
                context,
            )
        };

        if result == 0 {
            return Err(WinHttpError::from_last_error("WinHttpSendRequest"));
        }
        Ok(())
    }

    /// Sends the HTTP request for streaming upload with unknown content length.
    /// This uses WINHTTP_IGNORE_REQUEST_TOTAL_LENGTH to allow WinHttpWriteData calls.
    pub(crate) fn send_chunked(&self, context: usize) -> Result<()> {
        // WINHTTP_IGNORE_REQUEST_TOTAL_LENGTH = 0xFFFFFFFF
        // This tells WinHTTP that we'll be streaming data with unknown length
        const WINHTTP_IGNORE_REQUEST_TOTAL_LENGTH: u32 = 0xFFFFFFFF;

        let result = unsafe {
            WinHttpSendRequest(
                self.as_raw(),
                std::ptr::null(),
                0,
                std::ptr::null(),
                0,
                WINHTTP_IGNORE_REQUEST_TOTAL_LENGTH,
                context,
            )
        };

        if result == 0 {
            return Err(WinHttpError::from_last_error("WinHttpSendRequest"));
        }
        Ok(())
    }

    /// Sends the HTTP request for streaming upload with a known total content length.
    /// The body data will be written separately using WinHttpWriteData.
    pub(crate) fn send_with_total_length(&self, total_length: u64, context: usize) -> Result<()> {
        let result = unsafe {
            WinHttpSendRequest(
                self.as_raw(),
                std::ptr::null(),
                0,
                std::ptr::null(),
                0,
                total_length as u32,
                context,
            )
        };

        if result == 0 {
            return Err(WinHttpError::from_last_error("WinHttpSendRequest"));
        }
        Ok(())
    }

    /// Receives the response headers.
    pub(crate) fn receive_response(&self) -> Result<()> {
        let result = unsafe { WinHttpReceiveResponse(self.as_raw(), std::ptr::null_mut()) };

        if result == 0 {
            return Err(WinHttpError::from_last_error("WinHttpReceiveResponse"));
        }
        Ok(())
    }

    /// Queries the status code from the response.
    pub(crate) fn query_status_code(&self) -> Result<u16> {
        let mut status_code: u32 = 0;
        let mut size = std::mem::size_of::<u32>() as u32;

        let result = unsafe {
            WinHttpQueryHeaders(
                self.as_raw(),
                WINHTTP_QUERY_STATUS_CODE | WINHTTP_QUERY_FLAG_NUMBER,
                std::ptr::null(),
                &mut status_code as *mut u32 as *mut std::ffi::c_void,
                &mut size,
                std::ptr::null_mut(),
            )
        };

        if result == 0 {
            return Err(WinHttpError::from_last_error(
                "WinHttpQueryHeaders (status)",
            ));
        }

        Ok(status_code as u16)
    }

    /// Queries the content length from the response.
    pub(crate) fn query_content_length(&self) -> Option<u64> {
        let mut content_length: u64 = 0;
        let mut size = std::mem::size_of::<u64>() as u32;

        let result = unsafe {
            WinHttpQueryHeaders(
                self.as_raw(),
                WINHTTP_QUERY_CONTENT_LENGTH | WINHTTP_QUERY_FLAG_NUMBER64,
                std::ptr::null(),
                &mut content_length as *mut u64 as *mut std::ffi::c_void,
                &mut size,
                std::ptr::null_mut(),
            )
        };

        if result != 0 {
            Some(content_length)
        } else {
            None
        }
    }

    /// Queries a specific header value.
    pub(crate) fn query_header(&self, header_name: &str) -> Result<Vec<String>> {
        let header_name_wide: Vec<u16> = header_name
            .encode_utf16()
            .chain(std::iter::once(0))
            .collect();
        let mut res = vec![];

        let mut next_value_idx = 0;
        loop {
            // First, query the required buffer size
            let mut size: u32 = 0;
            let result = unsafe {
                WinHttpQueryHeaders(
                    self.as_raw(),
                    WINHTTP_QUERY_CUSTOM,
                    header_name_wide.as_ptr(),
                    std::ptr::null_mut(),
                    &mut size,
                    &mut next_value_idx,
                )
            };

            if result == 0 {
                let error = unsafe { windows_sys::Win32::Foundation::GetLastError() };
                if error == windows_sys::Win32::Foundation::ERROR_INSUFFICIENT_BUFFER {
                    // Buffer size is now in `size`, proceed to allocate
                } else if error
                    == windows_sys::Win32::Networking::WinHttp::ERROR_WINHTTP_HEADER_NOT_FOUND
                {
                    return Ok(res);
                } else {
                    return Err(WinHttpError::from_code(
                        error,
                        "WinHttpQueryHeaders (size query)",
                    ));
                }
            }

            // Allocate buffer and query the actual value
            let mut buffer: Vec<u16> = vec![0; (size / 2) as usize + 1];
            let result = unsafe {
                WinHttpQueryHeaders(
                    self.as_raw(),
                    WINHTTP_QUERY_CUSTOM,
                    header_name_wide.as_ptr(),
                    buffer.as_mut_ptr() as *mut std::ffi::c_void,
                    &mut size,
                    &mut next_value_idx,
                )
            };

            if result == 0 {
                return Err(WinHttpError::from_last_error("WinHttpQueryHeaders (value)"));
            }

            // Convert to string
            let value = String::from_utf16_lossy(&buffer[..(size / 2) as usize]);
            res.push(value);
        }
    }

    /// Queries available data to read.
    pub(crate) fn query_data_available(&self) -> Result<u32> {
        let mut available: u32 = 0;
        let result =
            unsafe { WinHttpQueryDataAvailable(self.as_raw(), &mut available as *mut u32) };

        if result == 0 {
            return Err(WinHttpError::from_last_error("WinHttpQueryDataAvailable"));
        }

        Ok(available)
    }

    /// Reads data from the response.
    pub(crate) fn read_data(&self, buffer: &mut [MaybeUninit<u8>]) -> Result<u32> {
        let mut bytes_read: u32 = 0;

        let result = unsafe {
            WinHttpReadData(
                self.as_raw(),
                buffer.as_mut_ptr() as *mut std::ffi::c_void,
                buffer.len() as u32,
                &mut bytes_read,
            )
        };

        if result == 0 {
            return Err(WinHttpError::from_last_error("WinHttpReadData"));
        }

        Ok(bytes_read)
    }

    /// Writes additional data for the request.
    /// # Safety
    /// The caller must ensure that the handle is in synchronous mode.
    pub(crate) unsafe fn write_data(&self, data: &[u8]) -> Result<u32> {
        let mut bytes_written: u32 = 0;

        let result = unsafe {
            WinHttpWriteData(
                self.as_raw(),
                data.as_ptr() as *const std::ffi::c_void,
                data.len() as u32,
                &mut bytes_written,
            )
        };

        if result == 0 {
            return Err(WinHttpError::from_last_error("WinHttpWriteData"));
        }

        Ok(bytes_written)
    }

    /// Sets an option on the request handle.
    ///
    /// # Safety
    ///
    /// The caller must ensure that the value pointer is valid and of the
    /// correct type for the specified option.
    pub(crate) unsafe fn set_option<T>(&self, option: u32, value: &T) -> Result<()> {
        let result = unsafe {
            WinHttpSetOption(
                self.as_raw(),
                option,
                value as *const T as *const std::ffi::c_void,
                std::mem::size_of::<T>() as u32,
            )
        };
        if result == 0 {
            return Err(WinHttpError::from_last_error("WinHttpSetOption"));
        }
        Ok(())
    }

    /// Ignores certificate errors on this request.
    pub(crate) fn ignore_certificate_errors(&self) -> Result<()> {
        let flags: u32 = SECURITY_FLAG_IGNORE_UNKNOWN_CA
            | SECURITY_FLAG_IGNORE_CERT_DATE_INVALID
            | SECURITY_FLAG_IGNORE_CERT_CN_INVALID
            | SECURITY_FLAG_IGNORE_CERT_WRONG_USAGE;
        unsafe { self.set_option(WINHTTP_OPTION_SECURITY_FLAGS, &flags) }
    }

    /// Disables automatic redirects on this request.
    pub(crate) fn disable_redirects(&self) -> Result<()> {
        let policy: u32 = WINHTTP_OPTION_REDIRECT_POLICY_NEVER;
        unsafe { self.set_option(WINHTTP_OPTION_REDIRECT_POLICY, &policy) }
    }

    /// Enables automatic redirects on this request.
    #[allow(dead_code)]
    pub(crate) fn enable_redirects(&self) -> Result<()> {
        let policy: u32 = WINHTTP_OPTION_REDIRECT_POLICY_ALWAYS;
        unsafe { self.set_option(WINHTTP_OPTION_REDIRECT_POLICY, &policy) }
    }

    /// Disables automatic cookies on this request.
    pub(crate) fn disable_cookies(&self) -> Result<()> {
        let flags: u32 = WINHTTP_DISABLE_COOKIES;
        unsafe { self.set_option(WINHTTP_OPTION_DISABLE_FEATURE, &flags) }
    }

    /// Sets the receive response timeout on this request (time to wait for server response).
    pub(crate) fn set_receive_response_timeout(&self, timeout_ms: u32) -> Result<()> {
        // Set both receive response timeout and receive timeout
        unsafe {
            self.set_option(WINHTTP_OPTION_RECEIVE_RESPONSE_TIMEOUT, &timeout_ms)?;
            self.set_option(WINHTTP_OPTION_RECEIVE_TIMEOUT, &timeout_ms)
        }
    }
}

impl Drop for RequestHandle {
    fn drop(&mut self) {
        unsafe {
            WinHttpCloseHandle(self.as_raw());
        }
    }
}
