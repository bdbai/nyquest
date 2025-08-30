use std::borrow::Cow;
use std::ffi::{c_char, CStr};
use std::pin::Pin;

use crate::curl_ng::error_context::{CurlCodeContext, WithCurlCodeContext as _};
use crate::curl_ng::raw_easy::RawEasy;

pub trait AsRawEasyMut {
    fn init(self: Pin<&mut Self>) -> Result<(), CurlCodeContext>;
    fn as_raw_easy_mut(self: Pin<&mut Self>) -> Pin<&mut RawEasy>;
    fn reset_extra(self: Pin<&mut Self>) -> Result<(), CurlCodeContext>;
}

impl<E: AsRawEasyMut + ?Sized> AsRawEasyMutExt for E {}

// TODO: move all methods to RawEasy
pub trait AsRawEasyMutExt: AsRawEasyMut {
    /// # Safety
    /// Callers must ensure that the error buffer is valid for the lifetime of
    /// the easy handle until it is detached.
    unsafe fn attach_error_buf(
        self: Pin<&mut Self>,
        error_buf: *mut c_char,
    ) -> Result<(), CurlCodeContext> {
        setopt_ptr(
            self.as_raw_easy_mut().raw(),
            curl_sys::CURLOPT_ERRORBUFFER,
            error_buf,
        )
        .with_easy_context("setopt CURLOPT_ERRORBUFFER")
    }
    fn detach_error_buf(self: Pin<&mut Self>) -> Result<(), CurlCodeContext> {
        let null_buf: *mut c_char = std::ptr::null_mut();
        setopt_ptr(
            self.as_raw_easy_mut().raw(),
            curl_sys::CURLOPT_ERRORBUFFER,
            null_buf,
        )
        .with_easy_context("setopt CURLOPT_ERRORBUFFER")
    }
    fn set_noproxy<'s>(
        self: Pin<&mut Self>,
        skip: impl Into<Cow<'s, str>>,
    ) -> Result<(), CurlCodeContext> {
        setopt_str(
            self.as_raw_easy_mut().raw(),
            curl_sys::CURLOPT_NOPROXY,
            skip.into(),
        )
        .with_easy_context("setopt CURLOPT_NOPROXY")
    }
}

// unsafe fn take_extra_err(&mut self, rc: curl_sys::CURLcode) -> Result<(), curl::Error> {
//     if rc == curl_sys::CURLE_OK {
//         return Ok(());
//     }
//     let mut err = curl::Error::new(rc);
//     // Safety: if the buffer is never written to, the first byte is
//     // guaranteed to be zero at the time of initialization.
//     let msg = unsafe {
//         CStr::from_ptr(self.error_buf.buf.as_ptr() as _)
//             .to_string_lossy()
//             .into_owned()
//     };
//     self.error_buf.buf[0].write(0);
//     if !msg.is_empty() {
//         err.set_extra(msg);
//     }
//     Err(err)
// }

fn setopt_str(
    raw: *mut curl_sys::CURL,
    opt: curl_sys::CURLoption,
    mut val: Cow<'_, str>,
) -> curl_sys::CURLcode {
    if val.ends_with('\0') {
        // Quick path: if the string ends with a null byte, we can just use
        // the pointer directly.
    } else {
        let mut s = val.into_owned();
        s.push('\0');
        val = Cow::Owned(s);
    };
    setopt_ptr(raw, opt, val.as_ptr() as *const c_char)
}

pub(super) fn setopt_ptr(
    raw: *mut curl_sys::CURL,
    opt: curl_sys::CURLoption,
    val: *const c_char,
) -> curl_sys::CURLcode {
    unsafe { curl_sys::curl_easy_setopt(raw, opt, val) }
}
