use thiserror::Error;

use crate::Error as BackendError;

#[derive(Debug, Error)]
#[non_exhaustive]
pub enum BuildClientError {
    #[error("No backend registered")]
    NoBackend,
    #[error("Error creating client: {0}")]
    BackendError(#[from] BackendError),
}

pub type BuildClientResult<T> = Result<T, BuildClientError>;
