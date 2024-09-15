use std::sync::OnceLock;

use super::any::{AnyBlockingBackend, AnyBlockingClient};
use crate::client::{BuildClientError, BuildClientResult, ClientBuilder};

pub(super) static BLOCKING_BACKEND_INSTANCE: OnceLock<Box<dyn AnyBlockingBackend>> =
    OnceLock::new();

pub struct BlockingClient {
    pub(super) client: Box<dyn AnyBlockingClient>,
}

impl ClientBuilder {
    pub fn build_blocking(self) -> BuildClientResult<crate::BlockingClient> {
        Ok(crate::BlockingClient {
            client: BLOCKING_BACKEND_INSTANCE
                .get()
                .ok_or(BuildClientError::NoBackend)?
                .create_blocking_client(self.options)?,
        })
    }
}

impl BlockingClient {
    pub fn get_string(&self, uri: impl Into<String>) -> crate::Result<String> {
        let req = crate::Request::new(uri.into(), "get".into());
        let mut res = self.client.request(req)?;
        res.text()
    }
}

impl Clone for BlockingClient {
    fn clone(&self) -> Self {
        Self {
            client: self.client.clone_boxed(),
        }
    }
}
