//! Blocking body types for HTTP requests.
//!
//! This module defines types for handling blocking request bodies.

use std::io::{Read, Seek};

/// Trait for blocking body streams.
pub trait BodyStream: Read + Seek + Send + 'static {}

/// Type alias for boxed blocking body streams.
pub type BoxedStream = Box<dyn BodyStream>;

/// Type alias for blocking HTTP request bodies.
pub type Body = crate::body::Body<BoxedStream>;

impl<S: Read + Seek + Send + 'static + ?Sized> BodyStream for S {}
