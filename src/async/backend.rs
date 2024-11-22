use std::future::Future;

use super::client::ASYNC_BACKEND_INSTANCE;
use super::BodyStream;
use crate::client::{BuildClientResult, ClientOptions};
use crate::Result;

pub fn register_async_backend(backend: impl AsyncBackend) {
    ASYNC_BACKEND_INSTANCE
        .set(Box::new(backend))
        .map_err(|_| ())
        .expect("nyquest async backend already registered");
}

pub trait AsyncClient: Clone + Send + Sync + 'static {
    type Response: AsyncResponse + Send;
    fn request(
        &self,
        req: crate::Request<BodyStream>,
    ) -> impl Future<Output = Result<Self::Response>> + Send;
    // TODO: fn request_with_progress
    // TODO: fn request_file
}

pub trait AsyncBackend: Send + Sync + 'static {
    type AsyncClient: AsyncClient;
    fn create_async_client(
        &self,
        options: ClientOptions,
    ) -> impl Future<Output = BuildClientResult<Self::AsyncClient>> + Send;
}

pub trait AsyncResponse: Send + Sync + 'static {
    fn status(&self) -> u16;
    fn content_length(&self) -> Option<u64>;
    fn get_header(&self, header: &str) -> Result<Vec<String>>;
    fn text(&mut self) -> impl Future<Output = Result<String>> + Send;
    fn bytes(&mut self) -> impl Future<Output = Result<Vec<u8>>> + Send;
}
