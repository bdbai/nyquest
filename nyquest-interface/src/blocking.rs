mod any;
mod backend;
mod body;

pub use any::{AnyBlockingBackend, AnyBlockingClient};
pub use backend::{BlockingBackend, BlockingClient, BlockingResponse};
pub use body::{Body, BoxedStream};
pub type Request = crate::Request<body::BoxedStream>;
