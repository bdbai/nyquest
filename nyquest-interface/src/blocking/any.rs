//! Type-erased blocking client interface traits.
//!
//! This module provides trait definitions for type-erased blocking HTTP client
//! implementations, allowing different backend implementations to be used interchangeably.
//!
//! The traits in this module are automatically implemented for types that implement the
//! corresponding traits from the `blocking::backend` module, so backend developers don't need
//! to implement them directly.

use std::any::Any;
use std::fmt;
use std::io;
use std::sync::Arc;

use super::backend::BlockingResponse;
use super::Request;
use crate::client::ClientOptions;
use crate::Result;

/// Trait for type-erased blocking backend implementations.
///
/// Automatically implemented for types implementing `BlockingBackend`.
pub trait AnyBlockingBackend: Send + Sync + 'static {
    /// Creates a new blocking client with the given options.
    fn create_blocking_client(&self, options: ClientOptions) -> Result<Arc<dyn AnyBlockingClient>>;
}

/// Trait for type-erased blocking HTTP clients.
///
/// Automatically implemented for types implementing `BlockingClient`.
pub trait AnyBlockingClient: Any + Send + Sync + 'static {
    /// Provides a textual description of this client.
    fn describe(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result;
    /// Sends an HTTP request and returns the response.
    fn request(&self, req: Request) -> crate::Result<Box<dyn AnyBlockingResponse>>;
}

/// Trait for type-erased blocking HTTP responses.
///
/// Automatically implemented for types implementing `BlockingResponse`.
/// It extends io::Read to allow streaming the response body.
pub trait AnyBlockingResponse: io::Read + Any + Send + Sync + 'static {
    /// Provides a textual description of this response.
    fn describe(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result;
    /// Returns the HTTP status code of this response.
    fn status(&self) -> u16;
    /// Returns the content-length of the response body, if known.
    fn content_length(&self) -> Option<u64>;
    /// Gets all values for the specified header.
    fn get_header(&self, header: &str) -> crate::Result<Vec<String>>;
    /// Reads the response body as text.
    fn text(&mut self) -> crate::Result<String>;
    /// Reads the response body as bytes.
    fn bytes(&mut self) -> crate::Result<Vec<u8>>;
}

// These implementations allow backend types implementing the base traits
// to be used with the type-erased trait system automatically.

impl<B> AnyBlockingBackend for B
where
    B: super::backend::BlockingBackend,
{
    fn create_blocking_client(&self, options: ClientOptions) -> Result<Arc<dyn AnyBlockingClient>> {
        Ok(Arc::new(self.create_blocking_client(options)?))
    }
}

impl<R> AnyBlockingResponse for R
where
    R: BlockingResponse,
{
    fn describe(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        BlockingResponse::describe(self, f)
    }

    fn status(&self) -> u16 {
        BlockingResponse::status(self)
    }

    fn content_length(&self) -> Option<u64> {
        BlockingResponse::content_length(self)
    }

    fn get_header(&self, header: &str) -> crate::Result<Vec<String>> {
        BlockingResponse::get_header(self, header)
    }

    fn text(&mut self) -> crate::Result<String> {
        BlockingResponse::text(self)
    }

    fn bytes(&mut self) -> crate::Result<Vec<u8>> {
        BlockingResponse::bytes(self)
    }
}

impl<B> AnyBlockingClient for B
where
    B: super::backend::BlockingClient,
{
    fn describe(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        super::backend::BlockingClient::describe(self, f)
    }
    fn request(&self, req: Request) -> crate::Result<Box<dyn AnyBlockingResponse>> {
        Ok(Box::new(self.request(req)?))
    }
}
