use thiserror::Error;

use nyquest_interface::Error as ErrorImpl;

/// The errors produced by the backend.
#[derive(Debug, Error)]
pub enum Error {
    /// The backend does not recognize the input as a valid URL.
    #[error("Invalid URL")]
    InvalidUrl,
    /// A generic backend error.
    #[error("IO Error")]
    Io(#[from] std::io::Error),
    /// Error occurred while serializing or deserializing JSON.
    #[cfg(feature = "json")]
    #[error("JSON ser/de Error")]
    #[cfg_attr(docsrs, doc(cfg(feature = "json")))]
    Json(#[from] serde_json::Error),
    /// The backend has received a response body that exceeds the maximum size limit specified in
    /// [`crate::ClientBuilder::max_response_buffer_size`].
    #[error("Response body size exceeds max limit")]
    ResponseTooLarge,
    /// The backend is not able to finish transferring the request within the timeout specified in
    /// [`crate::ClientBuilder::request_timeout`].
    #[error("Request is not finished within timeout")]
    RequestTimeout,
}

/// A `Result` alias where the `Err` case is [`crate::Error`].
pub type Result<T> = std::result::Result<T, Error>;

impl From<ErrorImpl> for Error {
    fn from(e: ErrorImpl) -> Self {
        match e {
            ErrorImpl::InvalidUrl => Self::InvalidUrl,
            ErrorImpl::Io(e) => Self::Io(e),
            ErrorImpl::ResponseTooLarge => Self::ResponseTooLarge,
            ErrorImpl::RequestTimeout => Self::RequestTimeout,
        }
    }
}
