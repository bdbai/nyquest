//! <style>
//! .rustdoc-hidden { display: none; }
//! </style>

#![doc = include_str!("../README.md")]
#![cfg_attr(docsrs, feature(doc_cfg))]

#[cfg(feature = "async")]
#[cfg_attr(docsrs, doc(cfg(feature = "async")))]
mod r#async;
mod backend;
#[cfg(feature = "blocking")]
#[cfg_attr(docsrs, doc(cfg(feature = "blocking")))]
mod blocking;
mod curl_ng;
mod custom;
mod error;
mod request;
mod state;
mod url;

pub use backend::CurlBackend;
pub use curl_ng::CurlCodeContext;
pub use curl_ng::CurlStringList;
pub use curl_sys;
pub use custom::{CurlHandle, HandleCustomizer};

/// Initializes libcurl.
pub fn init() {
    curl::init();
}

/// Initializes libcurl and registers the backend as global default.
pub fn register() {
    init();
    nyquest_interface::register_backend(CurlBackend::default());
}

/// Initializes libcurl and registers the backend with a customizer as global default.
pub fn register_custom(customizer: impl HandleCustomizer + Send + Sync + 'static) {
    init();
    nyquest_interface::register_backend(CurlBackend::customize(customizer));
}
