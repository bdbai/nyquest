use thiserror::Error;

use nyquest_interface::Error as ErrorImpl;

#[derive(Debug, Error)]
pub enum Error {
    #[error("Invalid URL")]
    InvalidUrl,
    #[error("IO Error")]
    Io(#[from] std::io::Error),
}

pub type Result<T> = std::result::Result<T, Error>;

impl From<ErrorImpl> for Error {
    fn from(e: ErrorImpl) -> Self {
        match e {
            ErrorImpl::InvalidUrl => Self::InvalidUrl,
            ErrorImpl::Io(e) => Self::Io(e),
        }
    }
}
