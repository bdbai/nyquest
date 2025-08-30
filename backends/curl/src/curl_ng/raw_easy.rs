use std::{pin::Pin, ptr::NonNull};

use crate::curl_ng::{easy_ref::AsRawEasyMut, error_context::CurlCodeContext};

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

    fn reset_extra(self: Pin<&mut Self>) -> Result<(), CurlCodeContext> {
        unsafe {
            curl_sys::curl_easy_reset(self.raw());
        }
        Ok(())
    }
}

impl Drop for RawEasy {
    fn drop(&mut self) {
        unsafe {
            curl_sys::curl_easy_cleanup(self.raw());
        }
    }
}
