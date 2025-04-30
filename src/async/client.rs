use std::fmt::Debug;

use nyquest_interface::{r#async::AnyAsyncClient, register::BACKEND};

use super::response::Response;
use crate::{
    client::{BuildClientError, BuildClientResult},
    ClientBuilder,
};

pub struct AsyncClient {
    pub(super) client: Box<dyn AnyAsyncClient>,
}

impl ClientBuilder {
    pub async fn build_async(self) -> BuildClientResult<AsyncClient> {
        Ok(AsyncClient {
            client: BACKEND
                .get()
                .ok_or(BuildClientError::NoBackend)?
                .create_async_client(self.options)
                .await?,
        })
    }
}

impl AsyncClient {
    pub async fn request(&self, req: super::Request) -> crate::Result<Response> {
        let res = self.client.request(req.inner).await?;
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

impl Debug for AsyncClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.client.describe(f)
    }
}
