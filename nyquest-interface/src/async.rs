//! Asynchronous HTTP client interface.
//!
//! This module provides the interfaces and types necessary for asynchronous
//! HTTP client implementations in nyquest.

mod any;
mod backend;
mod body;

pub use any::{AnyAsyncBackend, AnyAsyncClient, AnyAsyncResponse};
pub use backend::{AsyncBackend, AsyncClient, AsyncResponse};
pub use body::{Body, BoxedStream};
/// Type alias for asynchronous HTTP requests.
pub type Request = crate::Request<body::BoxedStream>;
pub use futures_io;
