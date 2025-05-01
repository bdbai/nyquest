//! Error types for nyquest HTTP operations.

use thiserror::Error;

/// Common error types that can occur in nyquest HTTP operations.
#[derive(Debug, Error)]
pub enum Error {
    /// The provided URL is invalid.
    #[error("Invalid URL")]
    InvalidUrl,
    /// An underlying I/O error occurred.
    #[error("IO Error")]
    Io(#[from] std::io::Error),
    /// The response body exceeds the maximum allowed size.
    #[error("Response body size exceeds max limit")]
    ResponseTooLarge,
    /// The request timed out before completion.
    #[error("Request is not finished within timeout")]
    RequestTimeout,
}

/// Result type for nyquest HTTP operations.
pub type Result<T> = std::result::Result<T, Error>;
