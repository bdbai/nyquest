use super::{any::AnyAsyncClient, response::Response};
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
            client: crate::register::BACKEND
                .get()
                .ok_or(BuildClientError::NoBackend)?
                .create_async_client(self.options)
                .await?,
        })
    }
}

impl AsyncClient {
    pub async fn request(&self, req: super::Request) -> crate::Result<Response> {
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
