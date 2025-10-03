use std::borrow::Cow;
use std::ffi::{c_int, c_uint};

#[derive(Debug, Clone)]
pub struct CurlCodeContext {
    pub(super) code: curl_sys::CURLcode,
    pub(super) context: &'static str,
}

#[derive(Debug, Clone)]
pub struct CurlErrorContext<'e> {
    pub code: curl_sys::CURLcode,
    pub msg: Cow<'e, str>,
    pub context: &'static str,
}

#[derive(Debug, Clone)]
pub struct CurlMultiCodeContext {
    pub code: curl_sys::CURLMcode,
    pub context: &'static str,
}

pub trait WithCurlCodeContext {
    fn with_easy_context(self, context: &'static str) -> Result<(), CurlCodeContext>;
    fn with_multi_context(self, context: &'static str) -> Result<(), CurlMultiCodeContext>;
}

impl WithCurlCodeContext for c_int {
    fn with_easy_context(self, context: &'static str) -> Result<(), CurlCodeContext> {
        (self as c_uint).with_easy_context(context)
    }
    fn with_multi_context(self, context: &'static str) -> Result<(), CurlMultiCodeContext> {
        if self == curl_sys::CURLM_OK {
            return Ok(());
        }
        Err(CurlMultiCodeContext {
            code: self,
            context,
        })
    }
}

impl WithCurlCodeContext for c_uint {
    fn with_easy_context(self, context: &'static str) -> Result<(), CurlCodeContext> {
        if self == 0 {
            return Ok(());
        }
        Err(CurlCodeContext {
            code: self as _,
            context,
        })
    }
    fn with_multi_context(self, _context: &'static str) -> Result<(), CurlMultiCodeContext> {
        unreachable!()
    }
}
