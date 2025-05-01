//! Error types for client building operations.

use thiserror::Error;

use crate::Error as BackendError;

/// Errors that can occur when building a nyquest HTTP client.
#[derive(Debug, Error)]
pub enum BuildClientError {
    /// No backend has been registered for nyquest.
    #[error("No backend registered")]
    NoBackend,
    /// An error occurred in the backend implementation.
    #[error("Error creating client: {0}")]
    BackendError(#[from] BackendError),
}

/// Result type for client building operations.
pub type BuildClientResult<T> = Result<T, BuildClientError>;
