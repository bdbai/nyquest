//! Blocking HTTP client interface.
//!
//! This module provides the interfaces and types necessary for blocking
//! HTTP client implementations in nyquest.

mod any;
mod backend;
mod body;

pub use any::{AnyBlockingBackend, AnyBlockingClient, AnyBlockingResponse};
pub use backend::{BlockingBackend, BlockingClient, BlockingResponse};
pub use body::{Body, BodyStream, BoxedStream};
/// Type alias for blocking HTTP requests.
pub type Request = crate::Request<body::BoxedStream>;
