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
            mod response;
            mod send_wrapper;
        }
    } else {
        #[cfg(feature = "async")]
        #[cfg_attr(docsrs, doc(cfg(feature = "async")))]
        mod r#async;
        #[cfg(feature = "blocking")]
        #[cfg_attr(docsrs, doc(cfg(feature = "blocking")))]
        mod blocking;
        mod response;
    }
}
mod client;
mod error;
mod request;

/// The backend implementation using reqwest.
pub struct ReqwestBackend;

/// Registers the reqwest backend as global default.
pub fn register() {
    nyquest_interface::register_backend(ReqwestBackend);
}
