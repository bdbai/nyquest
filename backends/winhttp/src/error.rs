//! Error handling for WinHTTP backend.

use std::fmt;
use std::io;

/// WinHTTP error wrapper.
#[derive(Debug)]
pub struct WinHttpError {
    code: u32,
    context: &'static str,
}

impl WinHttpError {
    /// Creates a new WinHttpError from the last Win32 error.
    pub(crate) fn from_last_error(context: &'static str) -> Self {
        let code = unsafe { windows_sys::Win32::Foundation::GetLastError() };
        Self { code, context }
    }

    /// Creates a new WinHttpError with a specific error code.
    pub(crate) fn from_code(code: u32, context: &'static str) -> Self {
        Self { code, context }
    }

    /// Returns the underlying Win32 error code.
    #[inline]
    pub fn code(&self) -> u32 {
        self.code
    }

    /// Returns the context where the error occurred.
    #[inline]
    pub fn context(&self) -> &'static str {
        self.context
    }
}

impl fmt::Display for WinHttpError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "WinHTTP error 0x{:08X} in {}", self.code, self.context)
    }
}

impl std::error::Error for WinHttpError {}

impl From<WinHttpError> for io::Error {
    fn from(err: WinHttpError) -> Self {
        io::Error::from_raw_os_error(err.code as i32)
    }
}

impl From<WinHttpError> for nyquest_interface::Error {
    fn from(err: WinHttpError) -> Self {
        use windows_sys::Win32::Networking::WinHttp::*;

        match err.code {
            ERROR_WINHTTP_TIMEOUT => nyquest_interface::Error::RequestTimeout,
            ERROR_WINHTTP_CANNOT_CONNECT
            | ERROR_WINHTTP_CONNECTION_ERROR
            | ERROR_WINHTTP_NAME_NOT_RESOLVED => nyquest_interface::Error::Io(io::Error::from(err)),
            ERROR_WINHTTP_SECURE_FAILURE => {
                nyquest_interface::Error::Io(io::Error::new(io::ErrorKind::InvalidData, err))
            }
            _ => nyquest_interface::Error::Io(io::Error::from(err)),
        }
    }
}

pub(crate) type Result<T> = std::result::Result<T, WinHttpError>;

/// Extension trait for converting WinHTTP results.
#[allow(dead_code)]
pub(crate) trait WinHttpResultExt<T> {
    fn into_nyquest(self) -> nyquest_interface::Result<T>;
}

impl<T> WinHttpResultExt<T> for Result<T> {
    fn into_nyquest(self) -> nyquest_interface::Result<T> {
        self.map_err(Into::into)
    }
}
