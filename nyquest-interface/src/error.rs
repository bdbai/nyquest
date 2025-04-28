use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("Invalid URL")]
    InvalidUrl,
    #[error("IO Error")]
    Io(#[from] std::io::Error),
    #[error("Response body size exceeds max limit")]
    ResponseTooLarge,
}

pub type Result<T> = std::result::Result<T, Error>;
