use std::fmt::Debug;

use nyquest_interface::{blocking::AnyBlockingClient, register::BACKEND};

use super::{response::Response, Request};
use crate::client::{BuildClientError, BuildClientResult, ClientBuilder};

/// A blocking HTTP client to make Requests with.
///
/// The current thread issuing an operation will be blocked until it is completed.
///
/// Depending on the backend implementation, it might holds a connection pool, a thread pool or
/// other kind of resources internally, so it is advised that you create one and reuse it to avoid
/// unnecessary overhead.
///
/// # Thread safety
///
/// The client is thread-safe and can be shared between threads.
///
/// Requests can be made and executed from multiple thread concurrently. The session, if any, will
/// be shared and synchronized between threads.
pub struct BlockingClient {
    pub(super) client: Box<dyn AnyBlockingClient>,
}

impl ClientBuilder {
    /// Build a new blocking client with the given options.
    pub fn build_blocking(self) -> BuildClientResult<BlockingClient> {
        Ok(BlockingClient {
            client: BACKEND
                .get()
                .ok_or(BuildClientError::NoBackend)?
                .create_blocking_client(self.options)?,
        })
    }
}

impl BlockingClient {
    /// Sends a request to the server and returns the response. The current thread will be blocked
    /// until the response is available or an error occurs.
    pub fn request(&self, req: Request) -> crate::Result<Response> {
        let res = self.client.request(req.inner)?;
        Ok(res.into())
    }

    // TODO: request file
}

impl Clone for BlockingClient {
    fn clone(&self) -> Self {
        Self {
            client: self.client.clone_boxed(),
        }
    }
}

impl Debug for BlockingClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.client.describe(f)
    }
}
