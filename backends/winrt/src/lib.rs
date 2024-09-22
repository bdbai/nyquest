#[cfg(all(windows, feature = "async"))]
pub mod r#async;
#[cfg(all(windows, feature = "blocking"))]
pub mod blocking;
mod client;
mod error;
mod response;
mod uri;

#[cfg(windows)]
pub fn register() {
    #[cfg(feature = "async")]
    nyquest::r#async::backend::register_async_backend(crate::r#async::WinrtAsyncBackend);
    #[cfg(feature = "blocking")]
    nyquest::blocking::backend::register_blocking_backend(crate::blocking::WinrtBlockingBackend);
}
