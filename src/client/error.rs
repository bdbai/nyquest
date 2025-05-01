use thiserror::Error;

use nyquest_interface::client::BuildClientError as BuildClientErrorImpl;

use crate::Error as BackendError;

/// The errors produced when building the backend.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum BuildClientError {
    /// No backend registered.
    #[error("No backend registered. Please find a backend crate (e.g. nyquest-preset) and call the `register` method at program startup.")]
    NoBackend,
    /// The backend has returned an error while creating the client.
    #[error("Error creating client: {0}")]
    BackendError(#[from] BackendError),
}

/// A `Result` alias where the `Err` case is [`BuildClientError`].
pub type BuildClientResult<T> = Result<T, BuildClientError>;

impl From<BuildClientErrorImpl> for BuildClientError {
    fn from(e: BuildClientErrorImpl) -> Self {
        match e {
            BuildClientErrorImpl::BackendError(e) => Self::BackendError(e.into()),
            BuildClientErrorImpl::NoBackend => Self::NoBackend,
        }
    }
}
