use std::io::ErrorKind;

use nyquest_interface::Result as NyquestResult;

pub(crate) trait IntoNyquestResult<T> {
    fn into_nyquest_result(self, ctx: &str) -> NyquestResult<T>;
}

impl<T> IntoNyquestResult<T> for Result<T, curl::Error> {
    fn into_nyquest_result(self, ctx: &str) -> NyquestResult<T> {
        // TODO: proper error mapping
        Ok(self.map_err(|e| {
            std::io::Error::new(
                ErrorKind::Other,
                format!("curl error:{}:{}", ctx, e.description()),
            )
        })?)
    }
}

impl<T> IntoNyquestResult<T> for Result<T, curl::MultiError> {
    fn into_nyquest_result(self, ctx: &str) -> NyquestResult<T> {
        // TODO: proper error mapping
        Ok(self.map_err(|e| {
            std::io::Error::new(
                ErrorKind::Other,
                format!("curl multi error:{}:{}", ctx, e.description()),
            )
        })?)
    }
}

impl<T> IntoNyquestResult<T> for Result<T, curl::ShareError> {
    fn into_nyquest_result(self, ctx: &str) -> NyquestResult<T> {
        // TODO: proper error mapping
        Ok(self.map_err(|e| {
            std::io::Error::new(
                ErrorKind::Other,
                format!("curl share error:{}:{}", ctx, e.description()),
            )
        })?)
    }
}
