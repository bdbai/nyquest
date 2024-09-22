use std::sync::OnceLock;

use super::{
    any::{AnyBlockingBackend, AnyBlockingClient},
    response::Response,
    Body,
};
use crate::client::{BuildClientError, BuildClientResult, ClientBuilder};

pub(super) static BLOCKING_BACKEND_INSTANCE: OnceLock<Box<dyn AnyBlockingBackend>> =
    OnceLock::new();

pub struct BlockingClient {
    pub(super) client: Box<dyn AnyBlockingClient>,
}

impl ClientBuilder {
    pub fn build_blocking(self) -> BuildClientResult<BlockingClient> {
        Ok(BlockingClient {
            client: BLOCKING_BACKEND_INSTANCE
                .get()
                .ok_or(BuildClientError::NoBackend)?
                .create_blocking_client(self.options)?,
        })
    }
}

impl BlockingClient {
    pub fn request(&self, req: crate::Request<Body>) -> crate::Result<Response> {
        let res = self.client.request(req)?;
        Ok(res.into())
    }

    // TODO: request file
}

impl Clone for BlockingClient {
    fn clone(&self) -> Self {
        Self {
            client: self.client.clone_boxed(),
        }
    }
}
