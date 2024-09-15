pub mod client;
mod error;
mod request;

#[cfg(feature = "async")]
pub mod r#async;
#[cfg(feature = "blocking")]
pub mod blocking;

#[cfg(feature = "blocking")]
pub use blocking::client::BlockingClient;
pub use client::ClientBuilder;
pub use error::{Error, Result};
pub use request::Request;
