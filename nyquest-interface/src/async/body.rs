//! Async body types for HTTP requests.
//!
//! This module defines types for handling asynchronous request bodies.

use std::io;
use std::pin::Pin;
use std::task::{Context, Poll};

use futures_io::{AsyncRead, AsyncSeek};

/// Trait for seekable async body streams with a known size.
pub trait SizedBodyStream: AsyncRead + AsyncSeek + Send + 'static {}

/// Trait for unsized async body streams that do not support seeking.
pub trait UnsizedBodyStream: AsyncRead + Send + 'static {}

/// A boxed async stream type that can either be sized or unsized.
pub enum BoxedStream {
    /// Sized stream with a known content length.
    Sized {
        /// The underlying stream that provides the body data.
        stream: Pin<Box<dyn SizedBodyStream>>,
        /// Content length of the stream.
        content_length: u64,
    },
    /// Unsized stream without a known content length.
    Unsized {
        /// The underlying stream that provides the body data.
        stream: Pin<Box<dyn UnsizedBodyStream>>,
    },
}

/// Type alias for asynchronous HTTP request bodies.
pub type Body = crate::body::Body<BoxedStream>;

impl<S: AsyncRead + AsyncSeek + Send + 'static + ?Sized> SizedBodyStream for S {}
impl<S: AsyncRead + Send + 'static + ?Sized> UnsizedBodyStream for S {}

impl BoxedStream {
    /// Get the total content length of the stream, if known.
    pub fn content_length(&self) -> Option<u64> {
        match self {
            BoxedStream::Sized { content_length, .. } => Some(*content_length),
            BoxedStream::Unsized { .. } => None,
        }
    }
}

impl AsyncRead for BoxedStream {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut [u8],
    ) -> Poll<io::Result<usize>> {
        match self.get_mut() {
            BoxedStream::Sized { stream, .. } => Pin::new(stream).poll_read(cx, buf),
            BoxedStream::Unsized { stream } => Pin::new(stream).poll_read(cx, buf),
        }
    }
}
