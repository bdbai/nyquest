//! Asynchronous HTTP client interface.
//!
//! This module provides the interfaces and types necessary for asynchronous
//! HTTP client implementations in nyquest.

mod any;
mod backend;

pub use any::{AnyAsyncBackend, AnyAsyncClient, AnyAsyncResponse};
pub use backend::{AsyncBackend, AsyncClient, AsyncResponse};
/// Type alias for asynchronous HTTP requests.
pub type Request = crate::Request<BoxedStream>;

cfg_if::cfg_if! {
    if #[cfg(feature = "async-stream")] {
        use futures_io::AsyncRead as MaybeAsyncRead;

        mod body;

        pub use body::{Body, BoxedStream, SizedBodyStream, UnsizedBodyStream};
        pub use futures_io;
    } else {
        /// Placeholder trait when async stream functionality is not required.
        pub trait MaybeAsyncRead {}
        impl<T: ?Sized> MaybeAsyncRead for T {}

        type BoxedStream = std::convert::Infallible;
        /// Type alias for common HTTP request bodies.
        pub type Body = crate::body::Body<std::convert::Infallible>;
    }
}
