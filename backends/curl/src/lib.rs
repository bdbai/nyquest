#[cfg(feature = "async")]
pub mod r#async;
#[cfg(feature = "blocking")]
pub mod blocking;
mod error;
mod request;
mod share;
mod url;
mod urlencoded;

pub struct CurlBackend;

pub fn init() {
    curl::init();
}
pub fn register() {
    init();
    nyquest_interface::register_backend(CurlBackend);
}
