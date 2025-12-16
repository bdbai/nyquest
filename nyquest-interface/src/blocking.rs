//! Blocking HTTP client interface.
//!
//! This module provides the interfaces and types necessary for blocking
//! HTTP client implementations in nyquest.

mod any;
mod backend;

pub use any::{AnyBlockingBackend, AnyBlockingClient, AnyBlockingResponse};
pub use backend::{BlockingBackend, BlockingClient, BlockingResponse};
/// Type alias for blocking HTTP requests.
pub type Request = crate::Request<BoxedStream>;

cfg_if::cfg_if! {
    if #[cfg(feature = "blocking-stream")] {
        use std::io::Read as MaybeRead;

        mod body;

        pub use body::{Body, BoxedStream, SizedBodyStream, UnsizedBodyStream};
    } else {
        /// Placeholder trait when blocking stream functionality is not required.
        pub trait MaybeRead {}
        impl<T: ?Sized> MaybeRead for T {}

        type BoxedStream = std::convert::Infallible;
        /// Type alias for common HTTP request bodies.
        pub type Body = crate::body::Body<std::convert::Infallible>;
    }
}
