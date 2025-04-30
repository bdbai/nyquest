use thiserror::Error;

use nyquest_interface::client::BuildClientError as BuildClientErrorImpl;

use crate::Error as BackendError;

#[derive(Debug, Error)]
#[non_exhaustive]
pub enum BuildClientError {
    #[error("No backend registered. Please find a backend crate (e.g. nyquest-preset) and call the `register` method at program startup.")]
    NoBackend,
    #[error("Error creating client: {0}")]
    BackendError(#[from] BackendError),
}

pub type BuildClientResult<T> = Result<T, BuildClientError>;

impl From<BuildClientErrorImpl> for BuildClientError {
    fn from(e: BuildClientErrorImpl) -> Self {
        match e {
            BuildClientErrorImpl::BackendError(e) => Self::BackendError(e.into()),
            BuildClientErrorImpl::NoBackend => Self::NoBackend,
        }
    }
}
