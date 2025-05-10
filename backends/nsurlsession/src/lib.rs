//! <style>
//! .rustdoc-hidden { display: none; }
//! </style>

#![doc = include_str!("../README.md")]
#![cfg_attr(docsrs, feature(doc_cfg))]

cfg_if::cfg_if! {
    if #[cfg(all(target_vendor = "apple", any(feature = "async", feature = "blocking")))] {
        #[cfg(feature = "async")]
        #[cfg_attr(docsrs, doc(cfg(feature = "async")))]
        mod r#async;
        #[cfg(feature = "blocking")]
        #[cfg_attr(docsrs, doc(cfg(feature = "blocking")))]
        mod blocking;

        mod challenge;
        mod client;
        mod datatask;
        mod error;
        #[cfg(feature = "multipart")]
        #[cfg_attr(docsrs, doc(cfg(feature = "multipart")))]
        mod multipart;
        mod response;
    }
}

/// The backend implementation using `NSURLSession`.
#[derive(Clone)]
pub struct NSUrlSessionBackend;

#[cfg(target_vendor = "apple")]
/// Registers [`NSUrlSessionBackend`] as global default.
pub fn register() {
    nyquest_interface::register_backend(NSUrlSessionBackend);
}
