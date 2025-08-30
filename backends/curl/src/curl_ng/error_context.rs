use std::borrow::Cow;

#[derive(Debug)]
pub struct CurlCodeContext {
    pub(super) code: curl_sys::CURLcode,
    pub(super) context: &'static str,
}

#[derive(Debug)]
pub struct CurlErrorContext<'e> {
    pub(super) code: curl_sys::CURLcode,
    pub(super) msg: Cow<'e, str>,
    pub(super) context: &'static str,
}

pub struct CurlMultiCodeContext {
    code: curl_sys::CURLMcode,
    context: &'static str,
}

pub trait WithCurlCodeContext {
    fn with_easy_context(self, context: &'static str) -> Result<(), CurlCodeContext>;
    fn with_multi_context(self, context: &'static str) -> Result<(), CurlMultiCodeContext>;
}

impl WithCurlCodeContext for curl_sys::CURLcode {
    fn with_easy_context(self, context: &'static str) -> Result<(), CurlCodeContext> {
        if self == curl_sys::CURLE_OK {
            return Ok(());
        }
        Err(CurlCodeContext {
            code: self,
            context,
        })
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
