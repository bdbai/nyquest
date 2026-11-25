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
//! - `async-ws`: Enable async WebSocket support
//! - `blocking`: Enable blocking interface support
//! - `blocking-stream`: Enable blocking interface and streaming upload/download support
//! - `blocking-ws`: Enable blocking WebSocket support
//! - `multipart`: Enable multipart form support
//! - `cookies`: Enable cookie management support
//! - `async-progress`: Enable async HTTP upload and download progress tracking
//! - `blocking-progress`: Enable blocking HTTP upload and download progress tracking
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
#[cfg(feature = "cookies")]
#[cfg_attr(docsrs, doc(cfg(feature = "cookies")))]
mod cookie;
mod error;
#[cfg(any(feature = "async-progress", feature = "blocking-progress"))]
#[cfg_attr(
    docsrs,
    doc(cfg(any(feature = "async-progress", feature = "blocking-progress")))
)]
mod progress;
#[doc(hidden)] // For nyquest facade only
pub mod register;
mod request;
#[cfg(any(feature = "async-ws", feature = "blocking-ws"))]
#[cfg_attr(docsrs, doc(cfg(any(feature = "async-ws", feature = "blocking-ws"))))]
pub mod websocket;

pub use body::Body;
#[cfg(feature = "multipart")]
#[cfg_attr(docsrs, doc(cfg(feature = "multipart")))]
pub use body::{Part, PartBody};
#[cfg(feature = "cookies")]
#[cfg_attr(docsrs, doc(cfg(feature = "cookies")))]
pub use cookie::Cookie;
pub use error::{Error, Result};
#[cfg(any(feature = "async-progress", feature = "blocking-progress"))]
#[cfg_attr(
    docsrs,
    doc(cfg(any(feature = "async-progress", feature = "blocking-progress")))
)]
pub use progress::{Progress, ProgressReporter};
pub use register::register_backend;
pub use request::{Method, Request};
#[cfg(any(feature = "async-ws", feature = "blocking-ws"))]
#[cfg_attr(docsrs, doc(cfg(any(feature = "async-ws", feature = "blocking-ws"))))]
pub use websocket::{WebSocketError, WebSocketInfo, WebSocketMessage, WebSocketResult};
