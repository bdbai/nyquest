//! Core async client interface traits.
//!
//! This module provides the core trait definitions that backend implementations
//! must implement to provide asynchronous HTTP functionality.
//!
//! Backend developers need to implement the `AsyncBackend` and `AsyncClient` traits,
//! along with a custom `AsyncResponse` type.

#[cfg(feature = "cookies")]
use std::borrow::Cow;
use std::fmt;
use std::future::Future;
use std::pin::Pin;
#[cfg(feature = "async-progress")]
use std::sync::Arc;

use super::Request as AsyncRequest;
use crate::client::ClientOptions;
#[cfg(feature = "async-progress")]
use crate::progress::ProgressReporter;
use crate::Result;

/// Trait for asynchronous HTTP clients.
///
/// Backend implementations must provide a concrete type that implements this trait
/// to handle asynchronous HTTP requests.
pub trait AsyncClient: Clone + Send + Sync + 'static {
    /// The type of response returned by this client.
    type Response: AsyncResponse + Send;

    /// Provides a textual description of this client.
    fn describe(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "AsyncClient")
    }

    /// Sends an HTTP request and returns a future that resolves to the response.
    fn request(&self, req: AsyncRequest) -> impl Future<Output = Result<Self::Response>> + Send;
    // TODO: fn request_file

    /// Sends an HTTP request with progress reporting.
    ///
    /// The `progress` reporter must remain available throughout response body
    /// consumption, not just request execution, so that download progress
    /// continues while the body is read.
    ///
    /// The default implementation delegates to [`AsyncClient::request`],
    /// ignoring the reporter, for backends that do not implement progress
    /// tracking.
    #[cfg(feature = "async-progress")]
    fn request_with_progress(
        &self,
        req: AsyncRequest,
        progress: Arc<dyn ProgressReporter>,
    ) -> impl Future<Output = Result<Self::Response>> + Send {
        async move {
            let _ = progress;
            self.request(req).await
        }
    }

    /// Opens a WebSocket connection.
    #[cfg(feature = "async-ws")]
    fn connect_websocket(
        &self,
        _uri: std::borrow::Cow<'static, str>,
        _protocols: Vec<std::borrow::Cow<'static, str>>,
    ) -> impl Future<Output = crate::WebSocketResult<super::websocket::WebSocket>> + Send {
        async { panic!("WebSocket is not supported by this backend") }
    }

    /// Adds a cookie for the given URL.
    #[cfg(feature = "cookies")]
    #[cfg_attr(docsrs, doc(cfg(feature = "cookies")))]
    fn add_cookie(
        &self,
        name: Cow<'static, str>,
        value: Cow<'static, str>,
        url: Cow<'static, str>,
    ) -> impl Future<Output = Result<()>> + Send;

    /// Returns the cookies that apply to the given URL.
    #[cfg(feature = "cookies")]
    #[cfg_attr(docsrs, doc(cfg(feature = "cookies")))]
    fn get_cookies_for(
        &self,
        url: Cow<'static, str>,
    ) -> impl Future<Output = Result<Vec<crate::Cookie>>> + Send;
}

/// Trait for asynchronous HTTP backend implementations.
///
/// This trait represents a backend implementation that can create async HTTP clients.
pub trait AsyncBackend: Send + Sync + 'static {
    /// The type of client this backend creates.
    type AsyncClient: AsyncClient;

    /// Creates a new async client with the given options.
    fn create_async_client(
        &self,
        options: ClientOptions,
    ) -> impl Future<Output = Result<Self::AsyncClient>> + Send;
}

/// Trait for asynchronous HTTP responses.
///
/// This trait provides methods for accessing the data in an HTTP response.
///
/// ## Response Method Receivers
///
/// Note that the `AsyncResponse` trait uses `&mut self` receivers for content reading methods
/// (`text()`, `bytes()`, etc.) to ensure object safety and dyn compatibility.
/// This differs from the main nyquest crate which uses consuming `self` receivers.
///
/// Backend implementors should design their implementations with the understanding that
/// these methods may be called only once per response instance, even though the signature allows
/// multiple calls. The nyquest facade enforces this by consuming the response.
pub trait AsyncResponse: super::MaybeAsyncRead + Send + Sync + 'static {
    /// Provides a textual description of this response.
    fn describe(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "AsyncResponse")
    }

    /// Returns the HTTP status code of this response.
    fn status(&self) -> u16;

    /// Returns the content-length of the response body, if known.
    fn content_length(&self) -> Option<u64>;

    /// Gets all values for the specified header.
    fn get_header(&self, header: &str) -> Result<Vec<String>>;

    /// Reads the response body as text.
    fn text(self: Pin<&mut Self>) -> impl Future<Output = Result<String>> + Send;

    /// Reads the response body as bytes.
    fn bytes(self: Pin<&mut Self>) -> impl Future<Output = Result<Vec<u8>>> + Send;
}
