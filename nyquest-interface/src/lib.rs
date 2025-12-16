//! Interface definitions for [`nyquest`] HTTP client backends.
//!
//! This crate provides the interface that backends must implement to be compatible with
//! the [`nyquest`] HTTP client facade. It defines the core types and traits that are used
//! across all nyquest implementations.
//!
//! ## Backend Registration
//!
//! Backend implementations must register themselves using the `register_backend` function
//! before they can be used by [`nyquest`].
//!
//! ## Features
//!
//! - `async`: Enable async interface support
//! - `async-stream`: Enable async interface and streaming upload/download support
//! - `blocking`: Enable blocking interface support
//! - `blocking-stream`: Enable blocking interface and streaming upload/download support
//! - `multipart`: Enable multipart form support
//!
//! [`nyquest`]: https://docs.rs/nyquest

#![cfg_attr(docsrs, feature(doc_cfg))]
#![deny(missing_docs)]

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

pub use body::Body;
#[cfg(feature = "multipart")]
#[cfg_attr(docsrs, doc(cfg(feature = "multipart")))]
pub use body::{Part, PartBody};
pub use error::{Error, Result};
pub use register::register_backend;
pub use request::{Method, Request};
