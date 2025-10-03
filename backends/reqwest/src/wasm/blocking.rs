use std::io::Read;

use nyquest_interface::blocking::{BlockingBackend, BlockingClient, BlockingResponse};

#[derive(Clone)]
pub struct DummyBlockingClient;
pub struct DummyBlockingResponse;

fn bail_unimplemented() -> ! {
    unimplemented!("blocking backend should not be used in wasm32 target");
}

impl Read for DummyBlockingResponse {
    fn read(&mut self, _buf: &mut [u8]) -> std::io::Result<usize> {
        bail_unimplemented()
    }
}

impl BlockingResponse for DummyBlockingResponse {
    fn status(&self) -> u16 {
        bail_unimplemented()
    }

    fn content_length(&self) -> Option<u64> {
        bail_unimplemented()
    }

    fn get_header(&self, _header: &str) -> nyquest_interface::Result<Vec<String>> {
        bail_unimplemented()
    }

    fn text(&mut self) -> nyquest_interface::Result<String> {
        bail_unimplemented()
    }

    fn bytes(&mut self) -> nyquest_interface::Result<Vec<u8>> {
        bail_unimplemented()
    }
}

impl BlockingClient for DummyBlockingClient {
    type Response = DummyBlockingResponse;

    fn request(
        &self,
        _req: nyquest_interface::blocking::Request,
    ) -> nyquest_interface::Result<Self::Response> {
        bail_unimplemented()
    }
}

impl BlockingBackend for crate::ReqwestBackend {
    type BlockingClient = DummyBlockingClient;

    fn create_blocking_client(
        &self,
        _options: nyquest_interface::client::ClientOptions,
    ) -> nyquest_interface::Result<Self::BlockingClient> {
        bail_unimplemented()
    }
}
