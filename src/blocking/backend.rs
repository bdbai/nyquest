use std::io;

use crate::client::{BuildClientResult, ClientOptions};

pub fn register_blocking_backend(backend: impl BlockingBackend) {
    super::client::BLOCKING_BACKEND_INSTANCE
        .set(Box::new(backend))
        .map_err(|_| ())
        .expect("nyquest blocking backend already registered");
}

pub trait BlockingClient: Clone + Send + Sync + 'static {
    type Response: BlockingResponse;
    fn request(&self, req: crate::Request) -> crate::Result<Self::Response>;
}

pub trait BlockingBackend: Send + Sync + 'static {
    type BlockingClient: BlockingClient;
    fn create_blocking_client(
        &self,
        options: ClientOptions,
    ) -> BuildClientResult<Self::BlockingClient>;
}

pub trait BlockingResponse: io::Read + Send + Sync + 'static {
    fn status(&self) -> u16;
    fn get_header(&self, header: &str) -> crate::Result<Vec<String>>;
    fn content_length(&self) -> crate::Result<Option<u64>>;
    fn text(&mut self) -> crate::Result<String>;
    fn bytes(&mut self) -> crate::Result<Vec<u8>>;
}
