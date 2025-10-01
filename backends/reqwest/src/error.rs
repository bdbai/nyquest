use std::io;

use nyquest_interface::Error as NyquestError;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum ReqwestBackendError {
    #[error("reqwest error: {0}")]
    Reqwest(#[from] reqwest::Error),
    #[cfg(not(target_arch = "wasm32"))]
    #[error("tokio error: {0}")]
    Tokio(#[from] tokio::task::JoinError),
    #[error("response too large")]
    ResponseTooLarge,
    #[error("invalid header name: {0}")]
    InvalidHeaderName(String),
    #[error("invalid header value: {0}")]
    InvalidHeaderValue(String),
    #[error("invalid URL: {0}")]
    InvalidUrl(String),
    #[error("invalid HTTP method")]
    InvalidMethod,
    #[cfg(target_arch = "wasm32")]
    #[error("unknown content type charset")]
    UnknownCharset,
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}

impl From<ReqwestBackendError> for NyquestError {
    fn from(err: ReqwestBackendError) -> Self {
        match err {
            ReqwestBackendError::Reqwest(e) => {
                if e.is_timeout() {
                    NyquestError::RequestTimeout
                } else {
                    NyquestError::Io(io::Error::other(e))
                }
            }
            ReqwestBackendError::ResponseTooLarge => NyquestError::ResponseTooLarge,
            ReqwestBackendError::InvalidUrl(msg) => {
                NyquestError::Io(io::Error::new(io::ErrorKind::InvalidInput, msg))
            }
            ReqwestBackendError::Io(e) => NyquestError::Io(e),
            other => NyquestError::Io(io::Error::other(other)),
        }
    }
}

pub type Result<T> = std::result::Result<T, ReqwestBackendError>;
