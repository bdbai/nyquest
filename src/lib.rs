mod error;
mod request;

#[cfg(feature = "async")]
pub mod r#async;
#[cfg(feature = "blocking")]
pub mod blocking;
pub(crate) mod body;
pub mod client;
mod register;

#[cfg(feature = "blocking")]
pub use blocking::client::BlockingClient;
#[doc(hidden)] // For backend implementation only
pub use body::Body;
#[doc(hidden)] // For backend implementation only
#[cfg(feature = "multipart")]
pub use body::PartBody;
pub use client::ClientBuilder;
pub use error::{Error, Result};
#[cfg(feature = "async")]
pub use r#async::client::AsyncClient;
pub use register::register_backend;
pub use request::Request;
