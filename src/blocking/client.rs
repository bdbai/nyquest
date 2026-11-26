#[cfg(any(feature = "blocking-ws", feature = "cookies"))]
use std::borrow::Cow;
use std::fmt::Debug;
use std::sync::Arc;

use nyquest_interface::{
    blocking::{AnyBlockingBackend, AnyBlockingClient},
    register::BACKEND,
};

#[cfg(feature = "blocking-ws")]
use super::websocket::WebSocket;
use super::{response::Response, Request};
use crate::client::ClientBuilder;
#[cfg(feature = "blocking-ws")]
use crate::websocket::WebSocketResult;

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
#[derive(Clone)]
pub struct BlockingClient {
    pub(super) client: Arc<dyn AnyBlockingClient>,
}

impl<Backend: AnyBlockingBackend> ClientBuilder<&Backend> {
    /// Build a new blocking client with the given options and custom backend.
    pub fn build_blocking(self) -> crate::Result<BlockingClient> {
        Ok(BlockingClient {
            client: self.backend.create_blocking_client(self.options)?,
        })
    }
}

impl ClientBuilder {
    /// Build a new blocking client with the given options.
    ///
    /// # Panic
    ///
    /// Panics if no backend is registered.
    pub fn build_blocking(self) -> crate::Result<BlockingClient> {
        Ok(BlockingClient {
            client: BACKEND
                .get()
                .expect("No backend registered. Please find a backend crate (e.g. nyquest-preset) and call the `register` method at program startup.")
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

    /// Sends a request with progress reporting.
    ///
    /// The `on_progress` callback is invoked with complete [`crate::Progress`]
    /// snapshots.
    #[cfg(feature = "blocking-progress")]
    pub fn request_with_progress(
        &self,
        req: Request,
        on_progress: impl Fn(crate::Progress) + Send + Sync + 'static,
    ) -> crate::Result<Response> {
        let reporter = crate::blocking::progress::CallbackProgressReporter::new(on_progress);
        let res = self
            .client
            .request_with_progress(req.inner, Arc::new(reporter))?;
        Ok(res.into())
    }

    /// Connects to a WebSocket endpoint.
    ///
    /// Received messages are limited by [`crate::ClientBuilder::max_response_buffer_size`].
    #[cfg(feature = "blocking-ws")]
    pub fn connect_websocket(
        &self,
        uri: impl Into<Cow<'static, str>>,
        protocols: impl IntoIterator<Item = Cow<'static, str>>,
    ) -> WebSocketResult<WebSocket> {
        Ok(WebSocket::from_interface(self.client.connect_websocket(
            uri.into(),
            protocols.into_iter().collect(),
        )?))
    }

    // TODO: request file

    /// Adds a cookie to the client's cookie store.
    ///
    /// The cookie is applied to requests whose URL matches `url` according to the cookie's domain
    /// and path.
    #[cfg(feature = "cookies")]
    pub fn add_cookie(
        &self,
        name: impl Into<Cow<'static, str>>,
        value: impl Into<Cow<'static, str>>,
        url: impl Into<Cow<'static, str>>,
    ) -> crate::Result<()> {
        Ok(self
            .client
            .add_cookie(name.into(), value.into(), url.into())?)
    }

    /// Returns the cookies that apply to `url`.
    #[cfg(feature = "cookies")]
    pub fn get_cookies_for(
        &self,
        url: impl Into<Cow<'static, str>>,
    ) -> crate::Result<Vec<crate::Cookie>> {
        Ok(self
            .client
            .get_cookies_for(url.into())?
            .into_iter()
            .map(Into::into)
            .collect())
    }
}

impl Debug for BlockingClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.client.describe(f)
    }
}
