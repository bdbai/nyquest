use std::future::Future;

use nyquest_interface::client::BuildClientResult;
use nyquest_interface::r#async::{AsyncBackend, AsyncClient, AsyncResponse};
use nyquest_interface::Result as NyquestResult;

use crate::NSUrlSessionBackend;

#[derive(Clone)]
pub struct NSUrlSessionAsyncClient {}

pub struct NSUrlSessionAsyncResponse {}

impl AsyncResponse for NSUrlSessionAsyncResponse {
    fn status(&self) -> u16 {
        todo!()
    }

    fn content_length(&self) -> Option<u64> {
        todo!()
    }

    fn get_header(&self, header: &str) -> NyquestResult<Vec<String>> {
        todo!()
    }

    fn text(&mut self) -> impl Future<Output = NyquestResult<String>> + Send {
        async { todo!() }
    }

    fn bytes(&mut self) -> impl Future<Output = NyquestResult<Vec<u8>>> + Send {
        async { todo!() }
    }
}

impl AsyncClient for NSUrlSessionAsyncClient {
    type Response = NSUrlSessionAsyncResponse;

    fn request(
        &self,
        req: nyquest_interface::r#async::Request,
    ) -> impl Future<Output = NyquestResult<Self::Response>> + Send {
        async { todo!() }
    }
}

impl AsyncBackend for NSUrlSessionBackend {
    type AsyncClient = NSUrlSessionAsyncClient;

    fn create_async_client(
        &self,
        options: nyquest_interface::client::ClientOptions,
    ) -> impl Future<Output = BuildClientResult<Self::AsyncClient>> + Send {
        async { todo!() }
    }
}
