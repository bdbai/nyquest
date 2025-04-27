use std::fmt;
use std::{any::Any, io};

use super::backend::BlockingResponse;
use super::Request;
use crate::client::{BuildClientResult, ClientOptions};

pub trait AnyBlockingBackend: Send + Sync + 'static {
    fn create_blocking_client(
        &self,
        options: ClientOptions,
    ) -> BuildClientResult<Box<dyn AnyBlockingClient>>;
}

pub trait AnyBlockingClient: Any + Send + Sync + 'static {
    fn describe(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result;
    fn clone_boxed(&self) -> Box<dyn AnyBlockingClient>;
    fn request(&self, req: Request) -> crate::Result<Box<dyn AnyBlockingResponse>>;
}

pub trait AnyBlockingResponse: io::Read + Any + Send + Sync + 'static {
    fn describe(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result;
    fn status(&self) -> u16;
    fn content_length(&self) -> Option<u64>;
    fn get_header(&self, header: &str) -> crate::Result<Vec<String>>;
    fn text(&mut self) -> crate::Result<String>;
    fn bytes(&mut self) -> crate::Result<Vec<u8>>;
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

impl<R> AnyBlockingResponse for R
where
    R: BlockingResponse,
{
    fn describe(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        BlockingResponse::describe(self, f)
    }

    fn status(&self) -> u16 {
        BlockingResponse::status(self)
    }

    fn content_length(&self) -> Option<u64> {
        BlockingResponse::content_length(self)
    }

    fn get_header(&self, header: &str) -> crate::Result<Vec<String>> {
        BlockingResponse::get_header(self, header)
    }

    fn text(&mut self) -> crate::Result<String> {
        BlockingResponse::text(self)
    }

    fn bytes(&mut self) -> crate::Result<Vec<u8>> {
        BlockingResponse::bytes(self)
    }
}

impl<B> AnyBlockingClient for B
where
    B: super::backend::BlockingClient,
{
    fn describe(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        super::backend::BlockingClient::describe(self, f)
    }
    fn clone_boxed(&self) -> Box<dyn AnyBlockingClient> {
        Box::new(self.clone())
    }
    fn request(&self, req: Request) -> crate::Result<Box<dyn AnyBlockingResponse>> {
        Ok(Box::new(self.request(req)?))
    }
}
