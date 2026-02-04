//! <style>
//! .rustdoc-hidden { display: none; }
//! </style>

#![doc = include_str!("../README.md")]
#![cfg_attr(docsrs, feature(doc_cfg))]

cfg_if::cfg_if! {
    if #[cfg(windows)] {
        #[cfg(feature = "async")]
        #[cfg_attr(docsrs, doc(cfg(feature = "async")))]
        mod r#async;
        #[cfg(feature = "blocking")]
        #[cfg_attr(docsrs, doc(cfg(feature = "blocking")))]
        mod blocking;

        mod error;
        mod handle;
        #[cfg(feature = "multipart")]
        #[cfg_attr(docsrs, doc(cfg(feature = "multipart")))]
        mod multipart;
        mod request;
        mod session;
        mod url;

        pub use error::WinHttpError;
    }
}

/// The backend implementation using WinHTTP.
#[derive(Clone)]
pub struct WinHttpBackend;

#[cfg(windows)]
/// Registers [`WinHttpBackend`] as global default.
pub fn register() {
    nyquest_interface::register_backend(WinHttpBackend);
}
