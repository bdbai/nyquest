use futures_core::future::BoxFuture;

use super::backend::AsyncResponse;
use super::BodyStream;
use crate::client::{BuildClientResult, ClientOptions};
use crate::{Request, Result};

pub trait AnyAsyncBackend: Send + Sync + 'static {
    fn create_async_client(
        &self,
        options: ClientOptions,
    ) -> BoxFuture<BuildClientResult<Box<dyn AnyAsyncClient>>>;
}

pub trait AnyAsyncClient: Send + Sync + 'static {
    fn clone_boxed(&self) -> Box<dyn AnyAsyncClient>;
    fn request(&self, req: Request<BodyStream>) -> BoxFuture<Result<Box<dyn AnyAsyncResponse>>>;
}

pub trait AnyAsyncResponse: Send + Sync + 'static {
    fn status(&self) -> u16;
    fn content_length(&self) -> Option<u64>;
    fn get_header(&self, header: &str) -> Result<Vec<String>>;
    fn text(&mut self) -> BoxFuture<Result<String>>;
    fn bytes(&mut self) -> BoxFuture<Result<Vec<u8>>>;
}

impl<R> AnyAsyncResponse for R
where
    R: AsyncResponse,
{
    fn status(&self) -> u16 {
        AsyncResponse::status(self)
    }

    fn content_length(&self) -> Option<u64> {
        AsyncResponse::content_length(self)
    }

    fn get_header(&self, header: &str) -> Result<Vec<String>> {
        AsyncResponse::get_header(self, header)
    }

    fn text(&mut self) -> BoxFuture<Result<String>> {
        Box::pin(AsyncResponse::text(self))
    }

    fn bytes(&mut self) -> BoxFuture<Result<Vec<u8>>> {
        Box::pin(AsyncResponse::bytes(self))
    }
}

impl<A> AnyAsyncBackend for A
where
    A: super::backend::AsyncBackend,
    A::AsyncClient: super::backend::AsyncClient,
{
    fn create_async_client(
        &self,
        options: ClientOptions,
    ) -> BoxFuture<BuildClientResult<Box<dyn AnyAsyncClient>>> {
        Box::pin(async {
            super::backend::AsyncBackend::create_async_client(self, options)
                .await
                .map(|client| Box::new(client) as Box<dyn AnyAsyncClient>)
        }) as _
    }
}

impl<A> AnyAsyncClient for A
where
    A: super::backend::AsyncClient,
{
    fn clone_boxed(&self) -> Box<dyn AnyAsyncClient> {
        Box::new(self.clone())
    }

    fn request(&self, req: Request<BodyStream>) -> BoxFuture<Result<Box<dyn AnyAsyncResponse>>> {
        Box::pin(async {
            self.request(req)
                .await
                .map(|res| Box::new(res) as Box<dyn AnyAsyncResponse>)
        }) as _
    }
}
