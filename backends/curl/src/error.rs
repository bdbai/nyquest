use nyquest_interface::{Error as NyquestError, Result as NyquestResult};

use crate::curl_ng::{error_context::CurlMultiCodeContext, CurlErrorContext};

pub(crate) trait IntoNyquestResult<T> {
    fn into_nyquest_result(self, ctx: &str) -> NyquestResult<T>;
}

impl<T> IntoNyquestResult<T> for Result<T, curl::Error> {
    fn into_nyquest_result(self, ctx: &str) -> NyquestResult<T> {
        // TODO: proper error mapping
        if self
            .as_ref()
            .err()
            .is_some_and(|e| e.is_operation_timedout())
        {
            return Err(NyquestError::RequestTimeout);
        }
        Ok(self.map_err(|e| {
            std::io::Error::other(format!("curl error:{}:{}", ctx, e.description()))
        })?)
    }
}

impl<T> IntoNyquestResult<T> for Result<T, curl::MultiError> {
    fn into_nyquest_result(self, ctx: &str) -> NyquestResult<T> {
        // TODO: proper error mapping
        Ok(self.map_err(|e| {
            std::io::Error::other(format!("curl multi error:{}:{}", ctx, e.description()))
        })?)
    }
}

impl<T> IntoNyquestResult<T> for Result<T, curl::ShareError> {
    fn into_nyquest_result(self, ctx: &str) -> NyquestResult<T> {
        // TODO: proper error mapping
        Ok(self.map_err(|e| {
            std::io::Error::other(format!("curl share error:{}:{}", ctx, e.description()))
        })?)
    }
}

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
