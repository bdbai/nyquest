//! Type-erased async client interface traits.
//!
//! This module provides trait definitions for type-erased asynchronous HTTP client
//! implementations, allowing different backend implementations to be used interchangeably.
//!
//! The traits in this module are automatically implemented for types that implement the
//! corresponding traits from the `async::backend` module, so backend developers don't need
//! to implement them directly.

use std::any::Any;
use std::fmt;
use std::pin::Pin;
use std::sync::Arc;

use futures_core::future::BoxFuture;
use futures_io::AsyncRead;

use super::backend::AsyncResponse;
use super::Request;
use crate::client::ClientOptions;
use crate::Result;

/// Trait for type-erased async backend implementations.
///
/// Automatically implemented for types implementing `AsyncBackend`.
pub trait AnyAsyncBackend: Send + Sync + 'static {
    /// Creates a new async client with the given options.
    fn create_async_client(
        &self,
        options: ClientOptions,
    ) -> BoxFuture<'_, Result<Arc<dyn AnyAsyncClient>>>;
}

/// Trait for type-erased async HTTP clients.
///
/// Automatically implemented for types implementing `AsyncClient`.
pub trait AnyAsyncClient: Any + Send + Sync + 'static {
    /// Provides a textual description of this client.
    fn describe(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result;
    /// Sends an HTTP request and returns the response.
    fn request(&self, req: Request) -> BoxFuture<'_, Result<Pin<Box<dyn AnyAsyncResponse>>>>;
}

/// Trait for type-erased async HTTP responses.
///
/// Automatically implemented for types implementing `AsyncResponse`.
pub trait AnyAsyncResponse: AsyncRead + Any + Send + Sync + 'static {
    /// Provides a textual description of this response.
    fn describe(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result;
    /// Returns the HTTP status code of this response.
    fn status(&self) -> u16;
    /// Returns the content-length of the response body, if known.
    fn content_length(&self) -> Option<u64>;
    /// Gets all values for the specified header.
    fn get_header(&self, header: &str) -> Result<Vec<String>>;
    /// Reads the response body as text.
    fn text(self: Pin<&mut Self>) -> BoxFuture<'_, Result<String>>;
    /// Reads the response body as bytes.
    fn bytes(self: Pin<&mut Self>) -> BoxFuture<'_, Result<Vec<u8>>>;
}

// These implementations allow backend types implementing the base traits
// to be used with the type-erased trait system automatically.

impl<R> AnyAsyncResponse for R
where
    R: AsyncResponse,
{
    fn status(&self) -> u16 {
        AsyncResponse::status(self)
    }

    fn content_length(&self) -> Option<u64> {
        AsyncResponse::content_length(self)
    }

    fn get_header(&self, header: &str) -> Result<Vec<String>> {
        AsyncResponse::get_header(self, header)
    }

    fn text(self: Pin<&mut Self>) -> BoxFuture<'_, Result<String>> {
        Box::pin(AsyncResponse::text(self))
    }

    fn bytes(self: Pin<&mut Self>) -> BoxFuture<'_, Result<Vec<u8>>> {
        Box::pin(AsyncResponse::bytes(self))
    }

    fn describe(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        AsyncResponse::describe(self, f)
    }
}

impl<A> AnyAsyncBackend for A
where
    A: super::backend::AsyncBackend,
    A::AsyncClient: super::backend::AsyncClient,
{
    fn create_async_client(
        &self,
        options: ClientOptions,
    ) -> BoxFuture<'_, Result<Arc<dyn AnyAsyncClient>>> {
        Box::pin(async {
            super::backend::AsyncBackend::create_async_client(self, options)
                .await
                .map(|client| Arc::new(client) as Arc<dyn AnyAsyncClient>)
        }) as _
    }
}

impl<A> AnyAsyncClient for A
where
    A: super::backend::AsyncClient,
{
    fn describe(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        super::backend::AsyncClient::describe(self, f)
    }

    fn request(&self, req: Request) -> BoxFuture<'_, Result<Pin<Box<dyn AnyAsyncResponse>>>> {
        Box::pin(async {
            self.request(req)
                .await
                .map(|res| Box::pin(res) as Pin<Box<dyn AnyAsyncResponse>>)
        }) as _
    }
}
