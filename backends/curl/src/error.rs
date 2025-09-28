use nyquest_interface::Error as NyquestError;

use crate::curl_ng::{error_context::CurlMultiCodeContext, CurlErrorContext};

impl<'e> From<CurlErrorContext<'e>> for NyquestError {
    fn from(e: CurlErrorContext<'e>) -> Self {
        if e.code == curl_sys::CURLE_OPERATION_TIMEDOUT {
            return NyquestError::RequestTimeout;
        }
        NyquestError::Io(std::io::Error::other(format!(
            "curl error:{}:{}:{}",
            e.context, e.code, e.msg
        )))
    }
}

impl From<CurlMultiCodeContext> for NyquestError {
    fn from(e: CurlMultiCodeContext) -> Self {
        if e.code == curl_sys::CURLE_OPERATION_TIMEDOUT {
            return NyquestError::RequestTimeout;
        }
        NyquestError::Io(std::io::Error::other(format!(
            "curl multi error:{}:{}",
            e.context, e.code
        )))
    }
}
