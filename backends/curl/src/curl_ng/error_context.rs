use std::borrow::Cow;

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
