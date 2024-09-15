use std::future::Future;
// use std::sync::OnceLock;

use crate::client::ClientOptions;
use crate::Result;

pub fn register_async_backend(backend: impl AsyncBackend) {
    todo!()
    // ASYNC_BACKEND_INSTANCE
    //     .set(Box::new(backend))
    //     .map_err(|_| ())
    //     .expect("nyquest async backend already registered");
}

pub trait AsyncClient: Clone + Send + Sync + 'static {
    type RequestFut: Future<Output = Result<String>> + Send;
    fn request(&self, method: &str, uri: &str, headers: (), body: Option<()>) -> Self::RequestFut;
    // TODO: fn request_with_progress
}

pub trait AsyncBackend: Send + Sync + 'static {
    type AsyncClient: AsyncClient;
    fn create_async_client(
        &self,
        options: ClientOptions,
    ) -> impl Future<Output = Self::AsyncClient> + Send;
}
