//! Blocking body types for HTTP requests.
//!
//! This module defines types for handling blocking request bodies.

use std::io::{Read, Seek};

/// Trait for blocking body streams.
#[doc(hidden)]
pub trait BodyStream: Read + Seek + Send {}

/// Type alias for boxed blocking body streams.
#[doc(hidden)]
pub type BoxedStream = Box<dyn BodyStream>;

/// Type alias for blocking HTTP request bodies.
pub type Body = crate::body::Body<BoxedStream>;

impl Body {
    /// Creates a new streaming body from a reader.
    #[doc(hidden)]
    pub fn stream<S: Read + Seek + Send + 'static>(stream: S, content_length: Option<u64>) -> Self {
        crate::body::Body::Stream(crate::body::StreamReader {
            stream: Box::new(stream),
            content_length,
        })
    }
}

impl<S: Read + Seek + Send + ?Sized> BodyStream for S {}
