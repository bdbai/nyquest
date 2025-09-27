use std::ffi::c_char;
use std::pin::Pin;
use std::ptr::NonNull;

use crate::curl_ng::easy::AsRawEasyMut;
use crate::curl_ng::{CurlCodeContext, WithCurlCodeContext as _};

#[derive(Debug)]
pub struct RawEasy {
    raw: NonNull<curl_sys::CURL>,
}

unsafe impl Send for RawEasy {}
unsafe impl Sync for RawEasy {}

impl RawEasy {
    pub fn raw(&self) -> *mut curl_sys::CURL {
        self.raw.as_ptr()
    }
}

impl AsRawEasyMut for RawEasy {
    fn init(self: Pin<&mut Self>) -> Result<(), CurlCodeContext> {
        Ok(())
    }

    fn as_raw_easy_mut(self: Pin<&mut Self>) -> Pin<&mut RawEasy> {
        self
    }

    fn reset(self: Pin<&mut Self>) -> Result<(), CurlCodeContext> {
        unsafe {
            curl_sys::curl_easy_reset(self.raw());
        }
        Ok(())
    }
}

impl RawEasy {
    /// # Safety
    /// Callers must ensure that the error buffer is valid for the lifetime of
    /// the easy handle until it is detached.
    pub(super) unsafe fn attach_error_buf(
        self: Pin<&mut Self>,
        error_buf: *mut c_char,
    ) -> Result<(), CurlCodeContext> {
        self.setopt_ptr(curl_sys::CURLOPT_ERRORBUFFER, error_buf)
            .with_easy_context("setopt CURLOPT_ERRORBUFFER")
    }
    pub(super) fn detach_error_buf(self: Pin<&mut Self>) -> Result<(), CurlCodeContext> {
        let null_buf: *mut c_char = std::ptr::null_mut();
        unsafe {
            self.setopt_ptr(curl_sys::CURLOPT_ERRORBUFFER, null_buf)
                .with_easy_context("setopt CURLOPT_ERRORBUFFER")
        }
    }
}

impl Drop for RawEasy {
    fn drop(&mut self) {
        unsafe {
            curl_sys::curl_easy_cleanup(self.raw());
        }
    }
}
