use std::borrow::Cow;
use std::ffi::{c_char, CStr};
use std::pin::Pin;

use crate::curl_ng::error_buf::{EasyWithErrorBuf, ErrorBuf, OwnedEasyWithErrorBuf};
use crate::curl_ng::raw_easy::RawEasy;

pub struct EasyWithErrorBufRef<'h, 'e> {
    easy: &'h mut RawEasy,
    error_buf: &'e mut ErrorBuf,
}

impl<'e> EasyWithErrorBuf<'e> {
    pub fn as_ref(&mut self) -> EasyWithErrorBufRef<'_, '_> {
        EasyWithErrorBufRef {
            easy: &mut self.easy,
            error_buf: &mut self.error_buf,
        }
    }
}

impl OwnedEasyWithErrorBuf {
    pub fn as_ref(self: Pin<&mut Self>) -> EasyWithErrorBufRef<'_, '_> {
        // Safety: Ref is guaranteed not to replace the underlying handle that
        // references to the error buffer.
        let this = unsafe { self.get_unchecked_mut() };
        EasyWithErrorBufRef {
            easy: &mut this.easy,
            error_buf: &mut this.error_buf,
        }
    }
}

impl<'h, 'e> EasyWithErrorBufRef<'h, 'e> {
    pub fn raw(&mut self) -> *mut curl_sys::CURL {
        self.easy.raw()
    }

    unsafe fn take_extra_err(&mut self, rc: curl_sys::CURLcode) -> Result<(), curl::Error> {
        if rc == curl_sys::CURLE_OK {
            return Ok(());
        }
        let mut err = curl::Error::new(rc);
        // Safety: if the buffer is never written to, the first byte is
        // guaranteed to be zero at the time of initialization.
        let msg = unsafe {
            CStr::from_ptr(self.error_buf.buf.as_ptr() as _)
                .to_string_lossy()
                .into_owned()
        };
        self.error_buf.buf[0].write(0);
        if !msg.is_empty() {
            err.set_extra(msg);
        }
        Err(err)
    }

    fn setopt_str(
        &mut self,
        opt: curl_sys::CURLoption,
        mut val: Cow<'_, str>,
    ) -> Result<(), curl::Error> {
        if val.ends_with('\0') {
            // Quick path: if the string ends with a null byte, we can just use
            // the pointer directly.
        } else {
            let mut s = val.into_owned();
            s.push('\0');
            val = Cow::Owned(s);
        };
        self.setopt_ptr(opt, val.as_ptr() as *const c_char)
    }

    fn setopt_ptr(
        &mut self,
        opt: curl_sys::CURLoption,
        val: *const c_char,
    ) -> Result<(), curl::Error> {
        unsafe { self.take_extra_err(curl_sys::curl_easy_setopt(self.easy.raw(), opt, val)) }
    }

    pub fn set_noproxy<'s>(&mut self, skip: impl Into<Cow<'s, str>>) -> Result<(), curl::Error> {
        self.setopt_str(curl_sys::CURLOPT_NOPROXY, skip.into())
    }
}
