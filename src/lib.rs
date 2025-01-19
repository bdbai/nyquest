mod error;
mod request;

#[cfg(feature = "async")]
pub mod r#async;
#[cfg(feature = "blocking")]
pub mod blocking;
pub mod body;
pub mod client;
mod register;

#[cfg(feature = "blocking")]
pub use blocking::client::BlockingClient;
pub use client::ClientBuilder;
pub use error::{Error, Result};
#[cfg(feature = "async")]
pub use r#async::client::AsyncClient;
pub use register::register_backend;
pub use request::Request;
