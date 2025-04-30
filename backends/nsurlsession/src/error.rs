use std::io::{self, ErrorKind};

use nyquest_interface::{Error as NyquestError, Result as NyquestResult};
use objc2::rc::{autoreleasepool, Retained};
use objc2_foundation::{NSError, NSURLErrorTimedOut};

pub(crate) trait IntoNyquestResult<T> {
    fn into_nyquest_result(self) -> NyquestResult<T>;
}

impl<T> IntoNyquestResult<T> for Result<T, Retained<NSError>> {
    fn into_nyquest_result(self) -> NyquestResult<T> {
        self.map_err(|e| {
            if e.code() == NSURLErrorTimedOut {
                return NyquestError::RequestTimeout;
            }
            let msg =
                autoreleasepool(|pool| unsafe { e.localizedDescription().to_str(pool).to_owned() });
            NyquestError::Io(io::Error::new(
                ErrorKind::Other,
                format!("NSURLSession error {}: {}", e.code(), msg),
            ))
        })
    }
}

impl IntoNyquestResult<()> for Option<Retained<NSError>> {
    fn into_nyquest_result(self) -> NyquestResult<()> {
        match self {
            Some(e) => Err(e).into_nyquest_result(),
            None => Ok(()),
        }
    }
}

impl<T> IntoNyquestResult<T> for NyquestResult<T> {
    fn into_nyquest_result(self) -> NyquestResult<T> {
        self
    }
}
