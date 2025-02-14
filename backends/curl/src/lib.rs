#[cfg(feature = "async")]
pub mod r#async;
#[cfg(feature = "blocking")]
pub mod blocking;
mod error;
mod request;
mod url;
mod urlencoded;

pub struct CurlBackend;

pub fn register() {
    curl::init();
    nyquest::register_backend(CurlBackend);
}
