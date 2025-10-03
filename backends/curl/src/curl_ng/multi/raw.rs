use std::ptr::NonNull;

use crate::curl_ng::multi::{IsSendWithMultiSet, IsSyncWithMultiSet};

#[derive(Debug)]
pub struct RawMulti {
    pub(super) raw: NonNull<curl_sys::CURLM>,
}

unsafe impl IsSendWithMultiSet for RawMulti {}
unsafe impl IsSyncWithMultiSet for RawMulti {}

impl RawMulti {
    pub fn new() -> Self {
        let raw = unsafe { curl_sys::curl_multi_init() };
        let raw = NonNull::new(raw).expect("curl_multi_init returned null");
        Self { raw }
    }

    pub fn raw(&self) -> *mut curl_sys::CURLM {
        self.raw.as_ptr()
    }
}

impl AsRef<RawMulti> for RawMulti {
    fn as_ref(&self) -> &RawMulti {
        self
    }
}

impl Drop for RawMulti {
    fn drop(&mut self) {
        unsafe {
            curl_sys::curl_multi_cleanup(self.raw());
        }
    }
}
