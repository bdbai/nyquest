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
mod client;
mod error;
mod request;
mod response;

use nyquest_interface::{register_backend, Result};

/// The backend implementation using reqwest.
pub struct ReqwestBackend;

#[cfg(feature = "async")]
impl nyquest_interface::r#async::AsyncBackend for ReqwestBackend {
    type AsyncClient = r#async::ReqwestAsyncClient;

    async fn create_async_client(
        &self,
        options: nyquest_interface::client::ClientOptions,
    ) -> Result<Self::AsyncClient> {
        r#async::ReqwestAsyncClient::new(options)
    }
}

#[cfg(feature = "blocking")]
impl nyquest_interface::blocking::BlockingBackend for ReqwestBackend {
    type BlockingClient = blocking::ReqwestBlockingClient;

    fn create_blocking_client(
        &self,
        options: nyquest_interface::client::ClientOptions,
    ) -> Result<Self::BlockingClient> {
        blocking::ReqwestBlockingClient::new(options)
    }
}

/// Registers the reqwest backend as global default.
pub fn register() {
    register_backend(ReqwestBackend);
}
