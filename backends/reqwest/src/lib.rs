//! <style>
//! .rustdoc-hidden { display: none; }
//! </style>

#![doc = include_str!("../README.md")]
#![cfg_attr(docsrs, feature(doc_cfg))]

cfg_if::cfg_if! {
    if #[cfg(target_arch = "wasm32")] {
        mod wasm {
            #[cfg(feature = "async")]
            #[cfg_attr(docsrs, doc(cfg(feature = "async")))]
            mod r#async;
            #[cfg(feature = "blocking")]
            #[cfg_attr(docsrs, doc(cfg(feature = "blocking")))]
            mod blocking;
            #[cfg(feature = "cookies")]
            mod cookies;
            mod options;
            mod response;
            mod send_wrapper;
            #[cfg(feature = "async-ws")]
            mod websocket;

            pub(crate) use options::WasmOptions;
        }
    } else {
        #[cfg(feature = "async")]
        #[cfg_attr(docsrs, doc(cfg(feature = "async")))]
        mod r#async;
        #[cfg(feature = "blocking")]
        #[cfg_attr(docsrs, doc(cfg(feature = "blocking")))]
        mod blocking;
        mod response;
        mod runtime;
    }
}
mod backend;
mod client;
#[cfg(feature = "cookies")]
mod cookie_util;
#[cfg(all(feature = "cookies", not(target_arch = "wasm32")))]
mod cookies;
mod customizer;
mod error;
#[cfg(all(
    any(feature = "async-progress", feature = "blocking-progress"),
    not(target_arch = "wasm32"),
    any(feature = "async", feature = "blocking")
))]
mod progress;
mod request;
#[cfg(all(
    any(feature = "async-ws", feature = "blocking-ws"),
    not(target_arch = "wasm32")
))]
mod websocket;

pub use crate::backend::ReqwestBackend;
#[doc(no_inline)]
pub use ::reqwest;

/// Registers the reqwest backend as global default.
pub fn register() {
    nyquest_interface::register_backend(ReqwestBackend::default());
}

/// Registers the reqwest backend with a customizer as global default.
pub fn register_custom(
    f: impl Fn(reqwest::ClientBuilder) -> reqwest::ClientBuilder + Send + Sync + 'static,
) {
    nyquest_interface::register_backend(ReqwestBackend::customize(f));
}
