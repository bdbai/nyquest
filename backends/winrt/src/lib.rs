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
        mod buffer;
        mod client;
        #[cfg(feature = "cookies")]
        mod cookies;
        mod error;
        mod ibuffer;
        mod request;
        mod response;
        #[cfg(not(any(feature = "async-progress", feature = "blocking-progress")))]
        mod response_size_limiter;
        mod string_pair;
        mod timer;
        mod uri;
        #[cfg(any(feature = "async-ws", feature = "blocking-ws"))]
        mod websocket;

        /// Registers [`WinrtBackend`] as global default.
        pub fn register() {
            nyquest_interface::register_backend(WinrtBackend);
        }
    }
}

/// The backend implementation using UWP/WinRT `HttpClient`.
#[derive(Clone)]
pub struct WinrtBackend;

// Unit-tested on all platforms; used by the backend on Windows.
#[allow(dead_code)]
#[cfg(any(feature = "async-progress", feature = "blocking-progress"))]
mod progress;
