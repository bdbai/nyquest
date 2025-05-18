//! Async body types for HTTP requests.
//!
//! This module defines types for handling asynchronous request bodies.

use futures_io::{AsyncRead, AsyncSeek};

/// Trait for asynchronous body streams.
pub trait BodyStream: AsyncRead + AsyncSeek + Send + 'static {}

/// Type alias for boxed asynchronous body streams.
pub type BoxedStream = Box<dyn BodyStream>;

/// Type alias for asynchronous HTTP request bodies.
pub type Body = crate::body::Body<BoxedStream>;

impl<S: AsyncRead + AsyncSeek + Send + 'static + ?Sized> BodyStream for S {}
