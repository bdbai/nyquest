//! Core blocking client interface traits.
//!
//! This module provides the core trait definitions that backend implementations
//! must implement to provide blocking HTTP functionality.
//!
//! Backend developers need to implement the `BlockingBackend` and `BlockingClient` traits,
//! along with a custom `BlockingResponse` type.

use std::{fmt, io};

use super::Request;
use crate::client::{BuildClientResult, ClientOptions};

/// Trait for blocking HTTP clients.
///
/// Backend implementations must provide a concrete type that implements this trait
/// to handle blocking HTTP requests.
pub trait BlockingClient: Clone + Send + Sync + 'static {
    /// The type of response returned by this client.
    type Response: BlockingResponse;

    /// Provides a textual description of this client.
    fn describe(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "BlockingClient")
    }

    /// Sends an HTTP request and returns the response.
    fn request(&self, req: Request) -> crate::Result<Self::Response>;
}

/// Trait for blocking HTTP backend implementations.
///
/// This trait represents a backend implementation that can create blocking HTTP clients.
pub trait BlockingBackend: Send + Sync + 'static {
    /// The type of client this backend creates.
    type BlockingClient: BlockingClient;

    /// Creates a new blocking client with the given options.
    fn create_blocking_client(
        &self,
        options: ClientOptions,
    ) -> BuildClientResult<Self::BlockingClient>;
}

/// Trait for blocking HTTP responses.
///
/// This trait provides methods for accessing the data in an HTTP response
/// and extends io::Read to allow streaming the response body.
///
/// ## Response Method Receivers
///
/// Note that the `BlockingResponse` trait uses `&mut self` receivers for content reading methods
/// (`text()`, `bytes()`, etc.) to ensure object safety and dyn compatibility.
/// This differs from the main nyquest crate which uses consuming `self` receivers.
///
/// Backend implementors should design their implementations with the understanding that
/// these methods may be called only once per response instance, even though the signature allows
/// multiple calls. The nyquest facade enforces this by consuming the response.
pub trait BlockingResponse: io::Read + Send + Sync + 'static {
    /// Provides a textual description of this response.
    fn describe(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "BlockingResponse")
    }

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
