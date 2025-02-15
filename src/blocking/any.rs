use super::backend::BlockingResponse;
use super::Request;
use crate::client::{BuildClientResult, ClientOptions};

pub trait AnyBlockingBackend: Send + Sync + 'static {
    fn create_blocking_client(
        &self,
        options: ClientOptions,
    ) -> BuildClientResult<Box<dyn AnyBlockingClient>>;
}

pub trait AnyBlockingClient: Send + Sync + 'static {
    fn clone_boxed(&self) -> Box<dyn AnyBlockingClient>;
    fn request(&self, req: Request) -> crate::Result<Box<dyn BlockingResponse>>;
}

impl<B> AnyBlockingBackend for B
where
    B: super::backend::BlockingBackend,
{
    fn create_blocking_client(
        &self,
        options: ClientOptions,
    ) -> BuildClientResult<Box<dyn AnyBlockingClient>> {
        Ok(Box::new(self.create_blocking_client(options)?))
    }
}

impl<B> AnyBlockingClient for B
where
    B: super::backend::BlockingClient,
{
    fn clone_boxed(&self) -> Box<dyn AnyBlockingClient> {
        Box::new(self.clone())
    }
    fn request(&self, req: Request) -> crate::Result<Box<dyn BlockingResponse>> {
        Ok(Box::new(self.request(req)?))
    }
}
