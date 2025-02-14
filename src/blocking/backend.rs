use std::io;

use super::Request;
use crate::client::{BuildClientResult, ClientOptions};

pub trait BlockingClient: Clone + Send + Sync + 'static {
    type Response: BlockingResponse;
    fn request(&self, req: Request) -> crate::Result<Self::Response>;
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
    fn content_length(&self) -> Option<u64>;
    fn get_header(&self, header: &str) -> crate::Result<Vec<String>>;
    fn text(&mut self) -> crate::Result<String>;
    fn bytes(&mut self) -> crate::Result<Vec<u8>>;
}
