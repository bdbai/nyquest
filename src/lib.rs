//! A truly native Rust HTTP client library.
//!
//! ## Overview
//!
//! Nyquest aims to fully utilize the functionality provided by existing HTTP libraries or built-in
//! HTTP stacks on various platforms, while providing a consistent and idiomatic API for Rust
//! developers. Based on the backend implementation, your application automatically benefits[^1] from
//!
//! - Core HTTP stack features
//! - Transparent response caching and session cookies
//! - Global proxy settings
//! - Hassle-free TLS
//! - Fewer Rust crate dependencies
//! - Smaller binary size
//! - System-managed security updates
//! - Better power management
//!
//! At the cost of
//!
//! - Abstraction and interop overhead
//! - Limited control over the underlying HTTP requests
//! - Potential inconsistencies in the behavior of different backends
//! - Link-time and runtime dependency to some native libraries (e.g. `libcurl` on Linux)
//!
//! ## The `nyquest` crate
//!
//! The `nyquest` crate is the main user interface of Nyquest HTTP clients for both library authors
//! and end users. It serves as a facade without actual implementations, and calls into the
//! preregistered backends via [`nyquest-interface`].
//!
//! ### Backends
//!
//! Before using `nyquest`, you need to register a backend. The simplest way is to use
//! [`nyquest-preset`] by adding it to your dependencies and calling the `register` function at
//! the beginning of your program. This will automatically select the appropriate backend based on
//! the target platform. Once registered, any transitive dependencies that use `nyquest` will also
//! pick up the registered backend given the version contraints of [`nyquest-interface`] are
//! compatible.
//!
//! You may want to handle the backends individually for more control. Currently, the
//! following backends are available:
//!
//! - [`nyquest-backend-winrt`](https://docs.rs/nyquest-backend-winrt)
//! - [`nyquest-backend-nsurlsession`](https://docs.rs/nyquest-backend-nsurlsession)
//! - [`nyquest-backend-curl`](https://docs.rs/nyquest-backend-curl)
//!
//! Refer to our [repository](https://github.com/bdbai/nyquest) for up-to-date
//! information on the backends.
//!
//! ### Threading and `async` Support
//!
//! Nyquest requires backends to be thread-safe in general. The "blocking" clients enabled by the
//! `blocking` feature can be used in any thread safely. The "async" clients enabled by the `async`
//! feature are thread-safe as well, and additionally the `Future`s they return are `Send`.
//!
//! An "async" client should not require an event loop or an async runtime available in the current
//! thread, allowing you to mix and match with any async runtimes. Under the hood, there may be
//! some threads managed by the backend crates or the HTTP stacks running in the background. With
//! that said, a backend implementation that would normally spin up its own event loop may decide
//! to reuse the one in the current thread if it is available, which may require certain features
//! of the runtime to be enabled.
//!
//! ## Usage
//!
//! Assume a backend with `async` feature has been registered. For a simple GET request, you can
//! use the shortcut `get` function:
//!
//! ```no_run
//! let body = nyquest::r#async::get("https://example.com").await?.text().await?;
//! println!("{body}");
//! ``````
//!
//! **Note**: If you plan to perform multiple requests, it is best to create a Client and reuse
//! it, taking advantage of thread reuse and potential keep-alive connection pooling.
//!
//! To send a POST request with urlencoded form data, use the `body_form!` macro to build the body:
//!
//! ```no_run
//! use nyquest::{body_form, ClientBuilder};
//! use nyquest::r#async::Request;
//! let client = ClientBuilder::default().build_async().await?;
//! let body = Request::post("http://httpbin.org/post").with_body(body_form! {
//!     "key1" => "value1",
//!     "key2" => "value2",
//! });
//! let resp = client.request(body).await?;
//! ```
//!
//! For blocking requests, simply change `r#async` to `blocking` and remove `.await`s in the above
//! examples.
//!
//! ## Features
//!
//! - `async`: Enable async support. The registered backend must implement the async interface
//! to compile.
//! - `blocking`: Enable blocking support. The registered backend must implement the blocking
//! interface to compile.
//! - `multipart`: Enable multipart form support. The registered backend must implement the
//! multipart interface to compile.
//! - `json`: Enable JSON request/response shorthand methods.
//!
//! [^1]: Subject to the backend's capability.
//!
//! [`nyquest-interface`]: https://docs.rs/nyquest-interface
//! [`nyquest-preset`]: https://docs.rs/nyquest-preset
//!

#![cfg_attr(docsrs, feature(doc_cfg))]
#![forbid(missing_docs)]

mod body;
mod error;
mod request;

#[cfg(feature = "async")]
#[cfg_attr(docsrs, doc(cfg(feature = "async")))]
pub mod r#async;
#[cfg(feature = "blocking")]
#[cfg_attr(docsrs, doc(cfg(feature = "blocking")))]
pub mod blocking;
pub mod client;

#[cfg(feature = "blocking")]
#[cfg_attr(docsrs, doc(cfg(feature = "blocking")))]
pub use blocking::client::BlockingClient;
#[cfg(feature = "multipart")]
#[cfg_attr(docsrs, doc(cfg(feature = "multipart")))]
pub use body::{Part, PartBody};
#[doc(inline)]
pub use client::ClientBuilder;
pub use error::{Error, Result};
#[cfg(feature = "async")]
#[cfg_attr(docsrs, doc(cfg(feature = "async")))]
pub use r#async::client::AsyncClient;
pub use request::{Method, Request};

#[doc(hidden)]
pub mod __private {
    pub use crate::body::Body;
}
