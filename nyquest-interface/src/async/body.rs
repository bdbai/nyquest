//! Async body types for HTTP requests.
//!
//! This module defines types for handling asynchronous request bodies.

use futures_io::{AsyncRead, AsyncSeek};

/// Trait for asynchronous body streams.
#[doc(hidden)]
pub trait BodyStream: AsyncRead + AsyncSeek + Send {}

/// Type alias for boxed asynchronous body streams.
#[doc(hidden)]
pub type BoxedStream = Box<dyn BodyStream>;

/// Type alias for asynchronous HTTP request bodies.
pub type Body = crate::body::Body<BoxedStream>;

impl Body {
    /// Creates a new streaming body from an async reader.
    #[doc(hidden)]
    pub fn stream<S: AsyncRead + AsyncSeek + Send + 'static>(
        stream: S,
        content_length: Option<u64>,
    ) -> Self {
        crate::body::Body::Stream(crate::body::StreamReader {
            stream: Box::new(stream),
            content_length,
        })
    }
}

impl<S: AsyncRead + AsyncSeek + Send + ?Sized> BodyStream for S {}
