use nyquest_interface::{Error as NyquestError, Result as NyquestResult};

pub(crate) trait IntoNyquestResult<T> {
    fn into_nyquest_result(self, ctx: &str) -> NyquestResult<T>;
}

impl<T> IntoNyquestResult<T> for Result<T, curl::Error> {
    fn into_nyquest_result(self, ctx: &str) -> NyquestResult<T> {
        // TODO: proper error mapping
        if self
            .as_ref()
            .err()
            .map(|e| e.is_operation_timedout())
            .is_some()
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
