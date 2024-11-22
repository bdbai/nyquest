use std::sync::OnceLock;

use super::{
    any::{AnyAsyncBackend, AnyAsyncClient},
    response::Response,
    BodyStream,
};
use crate::{
    client::{BuildClientError, BuildClientResult},
    ClientBuilder,
};

pub(super) static ASYNC_BACKEND_INSTANCE: OnceLock<Box<dyn AnyAsyncBackend>> = OnceLock::new();

pub struct AsyncClient {
    pub(super) client: Box<dyn AnyAsyncClient>,
}

impl ClientBuilder {
    pub async fn build_async(self) -> BuildClientResult<AsyncClient> {
        Ok(AsyncClient {
            client: ASYNC_BACKEND_INSTANCE
                .get()
                .ok_or(BuildClientError::NoBackend)?
                .create_async_client(self.options)
                .await?,
        })
    }
}

impl AsyncClient {
    pub async fn request(&self, req: crate::Request<BodyStream>) -> crate::Result<Response> {
        let res = self.client.request(req).await?;
        Ok(res.into())
    }
}

impl Clone for AsyncClient {
    fn clone(&self) -> Self {
        Self {
            client: self.client.clone_boxed(),
        }
    }
}
