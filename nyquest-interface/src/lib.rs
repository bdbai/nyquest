#![cfg_attr(docsrs, feature(doc_cfg))]

#[cfg(feature = "async")]
#[cfg_attr(docsrs, doc(cfg(feature = "async")))]
pub mod r#async;
#[cfg(feature = "blocking")]
#[cfg_attr(docsrs, doc(cfg(feature = "blocking")))]
pub mod blocking;
pub mod body;
pub mod client;
mod error;
#[doc(hidden)] // For nyquest facade only
pub mod register;
mod request;

pub use body::{Body, StreamReader};
#[cfg(feature = "multipart")]
#[cfg_attr(docsrs, doc(cfg(feature = "multipart")))]
pub use body::{Part, PartBody};
pub use error::{Error, Result};
pub use register::register_backend;
pub use request::{Method, Request};
