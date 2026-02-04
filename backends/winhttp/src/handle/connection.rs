//! WinHTTP connection handle wrapper.

use std::ptr::NonNull;

use windows_sys::Win32::Networking::WinHttp::*;

use super::session::SessionHandle;
use crate::error::{Result, WinHttpError};

/// A wrapper around WinHTTP connection handle (HINTERNET from WinHttpConnect).
///
/// The connection handle represents a connection to a specific server.
#[derive(Debug)]
pub(crate) struct ConnectionHandle {
    handle: NonNull<std::ffi::c_void>,
}

unsafe impl Send for ConnectionHandle {}
unsafe impl Sync for ConnectionHandle {}

impl ConnectionHandle {
    /// Creates a new connection to the specified server.
    pub(crate) fn connect(session: &SessionHandle, host: &str, port: u16) -> Result<Self> {
        let host_wide: Vec<u16> = host.encode_utf16().chain(std::iter::once(0)).collect();

        let handle = unsafe { WinHttpConnect(session.as_raw(), host_wide.as_ptr(), port, 0) };

        NonNull::new(handle)
            .map(|handle| Self { handle })
            .ok_or_else(|| WinHttpError::from_last_error("WinHttpConnect"))
    }

    /// Returns the raw handle.
    #[inline]
    pub(crate) fn as_raw(&self) -> *mut std::ffi::c_void {
        self.handle.as_ptr()
    }
}

impl Drop for ConnectionHandle {
    fn drop(&mut self) {
        unsafe {
            WinHttpCloseHandle(self.as_raw());
        }
    }
}
