use std::{future::Future, sync::Arc};

use nyquest::client::BuildClientResult;

pub struct CurlMultiBackend;

#[derive(Clone)]
pub struct CurlMultiClient {
    options: Arc<nyquest::client::ClientOptions>,
}

impl nyquest::r#async::backend::AsyncClient for CurlMultiClient {
    type Response = ;

    fn request(
        &self,
        req: nyquest::Request<nyquest::r#async::Body>,
    ) -> impl Future<Output = nyquest::Result<Self::Response>> + Send {
        todo!()
    }
}

impl nyquest::r#async::backend::AsyncBackend for CurlMultiBackend {
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
