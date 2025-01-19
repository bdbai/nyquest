use std::{future::Future, sync::Arc};

use nyquest::{client::BuildClientResult, r#async::backend::AsyncResponse};

#[derive(Clone)]
pub struct CurlMultiClient {
    options: Arc<nyquest::client::ClientOptions>,
}

pub struct CurlAsyncResponse {}

impl AsyncResponse for CurlAsyncResponse {
    fn status(&self) -> u16 {
        todo!()
    }

    fn content_length(&self) -> Option<u64> {
        todo!()
    }

    fn get_header(&self, header: &str) -> nyquest::Result<Vec<String>> {
        todo!()
    }

    async fn text(&mut self) -> nyquest::Result<String> {
        todo!()
    }

    async fn bytes(&mut self) -> nyquest::Result<Vec<u8>> {
        todo!()
    }
}

impl nyquest::r#async::backend::AsyncClient for CurlMultiClient {
    type Response = CurlAsyncResponse;

    async fn request(
        &self,
        _req: nyquest::Request<nyquest::r#async::BodyStream>,
    ) -> nyquest::Result<Self::Response> {
        todo!()
    }
}

impl nyquest::r#async::backend::AsyncBackend for crate::CurlBackend {
    type AsyncClient = CurlMultiClient;

    fn create_async_client(
        &self,
        options: nyquest::client::ClientOptions,
    ) -> impl Future<Output = BuildClientResult<Self::AsyncClient>> + Send {
        async {
            Ok(CurlMultiClient {
                options: Arc::new(options),
            })
        }
    }
}
