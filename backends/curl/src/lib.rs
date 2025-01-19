#[cfg(feature = "async")]
pub mod r#async;
#[cfg(feature = "blocking")]
pub mod blocking;
mod error;
#[cfg(feature = "blocking")]
mod multi_easy;
mod url;

pub struct CurlBackend;

pub fn register() {
    nyquest::register_backend(CurlBackend);
}
