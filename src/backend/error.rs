use thiserror::Error;

#[derive(Debug, Error)]
pub enum BackendError {
    #[error("Invalid URL")]
    InvalidUrl,
}

pub type BackendResult<T> = Result<T, BackendError>;
