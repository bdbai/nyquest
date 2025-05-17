use std::fmt::Debug;

use nyquest_interface::{r#async::AnyAsyncClient, register::BACKEND};

use super::response::Response;
use crate::ClientBuilder;

/// A async HTTP client to make Requests with.
///
/// See the [crate#threading-and-async-support]-level documentation for async runtime considerations.
///
/// Depending on the backend implementation, it might holds a connection pool, a thread pool or
/// other kind of resources internally, so it is advised that you create one and reuse it to avoid
/// unnecessary overhead.
pub struct AsyncClient {
    pub(super) client: Box<dyn AnyAsyncClient>,
}

impl ClientBuilder {
    /// Build a new async client with the given options.
    ///
    /// # Panic
    ///
    /// Panics if no backend is registered.
    pub async fn build_async(self) -> crate::Result<AsyncClient> {
        Ok(AsyncClient {
            client: BACKEND
                .get()
                .expect("No backend registered. Please find a backend crate (e.g. nyquest-preset) and call the `register` method at program startup.")
                .create_async_client(self.options)
                .await?
        })
    }
}

impl AsyncClient {
    /// Sends a request to the server and returns the response.
    pub async fn request(&self, req: super::Request) -> crate::Result<Response> {
        let res = self.client.request(req.inner).await?;
        Ok(res.into())
    }
}

impl Clone for AsyncClient {
    fn clone(&self) -> Self {
        Self {
            client: self.client.clone_boxed(),
        }
    }
}

impl Debug for AsyncClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.client.describe(f)
    }
}
