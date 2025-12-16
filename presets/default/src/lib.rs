//! Nyquest preset configuration with up-to-date rich-featured backends.
//!
//! `nyquest-preset` is the official, default backend provider of [`nyquest`] that integrates
//! [`nyquest-backend-winrt`], [`nyquest-backend-nsurlsession`] and [`nyquest-backend-curl`]
//! into a uniform interface. The only exposed APIs are the [`register`] function and the
//! [`Backend`] type of the underlying backend.
//!
//! This crate is intended to be consumed by end application users. Since there can be only one
//! backend registered as the global default, library authors in general are not recommended to
//! declare this crate as a dependency. Libraries should use [`nyquest`] instead.
//!
//! ## Quick Start
//!
//! Add the following at your program startup:
//! ```no_run
//! nyquest_backend::register();
//! ```
//! Based on the target platform, a [`nyquest`] backend will be registered as the default. Refer to
//! the documentation of [`nyquest`] for usages.
//!
//! ## Platform Support
//!
//! `nyquest-preset` uses `cfg` to select the appropriate backend for the target platform.
//!
//! - `windows`: [`nyquest-backend-winrt`]
//! - `target_vendor = "apple"`: [`nyquest-backend-nsurlsession`]
//! - others: [`nyquest-backend-curl`]
//!
//! Refer to the backends' documentation for specific platform requirements.
//!
//! ## Features
//!
//! - `async`: Enable async support for backends and [`nyquest`].
//! - `async-stream`: Enable async support and streaming upload/download for backends and
//!   [`nyquest`].
//! - `blocking`: Enable blocking support for backends and [`nyquest`].
//! - `blocking-stream`: Enable blocking support and streaming upload/download for backends and
//!   [`nyquest`].
//! - `multipart`: Enable multipart form support for backends and [`nyquest`].
//!
//! Refer to the backends' documentation for more optional features. For example, enable
//! `charset-defaults` for [`nyquest-backend-curl`] to perform encoding conversion automatically
//! when the response has an encoding other than UTF-8.
//!
//! [`nyquest-backend-winrt`]: https://docs.rs/nyquest-backend-winrt
//! [`nyquest-backend-nsurlsession`]: https://docs.rs/nyquest-backend-nsurlsession
//! [`nyquest-backend-curl`]: https://docs.rs/nyquest-backend-curl
//!
mod sys;

pub use nyquest;

/// Initialize and register the underlying backend as global default.
pub use sys::register;
#[doc(no_inline)]
pub use sys::Backend;

#[cfg(feature = "auto-register")]
ctor::declarative::ctor! {
    #[ctor]
    fn auto_register() {
        register();
    }
}
