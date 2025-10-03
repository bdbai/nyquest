//! <style>
//! .rustdoc-hidden { display: none; }
//! </style>

#![doc = include_str!("../README.md")]
#![cfg_attr(docsrs, feature(doc_cfg))]

#[cfg(feature = "async")]
#[cfg_attr(docsrs, doc(cfg(feature = "async")))]
mod r#async;
#[cfg(feature = "blocking")]
#[cfg_attr(docsrs, doc(cfg(feature = "blocking")))]
mod blocking;
mod curl_ng;
mod error;
mod request;
mod state;
mod url;

/// The backend implementation using libcurl.
pub struct CurlBackend;

/// Initializes libcurl.
pub fn init() {
    curl::init();
}

/// Initializes libcurl and registers the backend as global default.
pub fn register() {
    init();
    nyquest_interface::register_backend(CurlBackend);
}
