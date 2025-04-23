mod body;
mod error;
mod request;

#[cfg(feature = "async")]
pub mod r#async;
#[cfg(feature = "blocking")]
pub mod blocking;
pub mod client;

#[cfg(feature = "blocking")]
pub use blocking::client::BlockingClient;
#[cfg(feature = "multipart")]
pub use body::{Part, PartBody};
pub use client::ClientBuilder;
pub use error::{Error, Result};
#[cfg(feature = "async")]
pub use r#async::client::AsyncClient;
pub use request::Request;

#[doc(hidden)]
pub mod __private {
    pub use crate::body::Body;
}
