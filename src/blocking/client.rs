use std::fmt::Debug;

use nyquest_interface::{blocking::AnyBlockingClient, register::BACKEND};

use super::{response::Response, Request};
use crate::client::{BuildClientError, BuildClientResult, ClientBuilder};

pub struct BlockingClient {
    pub(super) client: Box<dyn AnyBlockingClient>,
}

impl ClientBuilder {
    pub fn build_blocking(self) -> BuildClientResult<BlockingClient> {
        Ok(BlockingClient {
            client: BACKEND
                .get()
                .ok_or(BuildClientError::NoBackend)?
                .create_blocking_client(self.options)?,
        })
    }
}

impl BlockingClient {
    pub fn request(&self, req: Request) -> crate::Result<Response> {
        let res = self.client.request(req.inner)?;
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

impl Debug for BlockingClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.client.describe(f)
    }
}
