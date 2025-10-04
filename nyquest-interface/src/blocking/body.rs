//! Blocking body types for HTTP requests.
//!
//! This module defines types for handling blocking request bodies.

use std::io::{Read, Seek};

/// Trait for seekable blocking body streams with a known size.
pub trait SizedBodyStream: Read + Seek + Send + 'static {}

/// Trait for unsized blocking body streams that do not support seeking.
pub trait UnsizedBodyStream: Read + Send + 'static {}

/// A boxed blocking stream type that can either be sized or unsized.
pub enum BoxedStream {
    /// Sized stream with a known content length.
    Sized {
        /// The underlying stream that provides the body data.
        stream: Box<dyn SizedBodyStream>,
        /// Content length of the stream.
        content_length: u64,
    },
    /// Unsized stream without a known content length.
    Unsized {
        /// The underlying stream that provides the body data.
        stream: Box<dyn UnsizedBodyStream>,
    },
}

/// Type alias for blocking HTTP request bodies.
pub type Body = crate::body::Body<BoxedStream>;

impl<S: Read + Seek + Send + 'static + ?Sized> SizedBodyStream for S {}
impl<S: Read + Send + 'static + ?Sized> UnsizedBodyStream for S {}

impl BoxedStream {
    /// Get the total content length of the stream, if known.
    pub fn content_length(&self) -> Option<u64> {
        match self {
            BoxedStream::Sized { content_length, .. } => Some(*content_length),
            BoxedStream::Unsized { .. } => None,
        }
    }
}

impl Read for BoxedStream {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        match self {
            BoxedStream::Sized { stream, .. } => stream.read(buf),
            BoxedStream::Unsized { stream } => stream.read(buf),
        }
    }
}
