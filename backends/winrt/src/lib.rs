#[cfg(all(windows, feature = "blocking"))]
pub mod blocking;
#[cfg(all(windows, feature = "async"))]
pub mod client;
mod error;
mod response;

#[cfg(windows)]
pub fn register() {
    // #[cfg(feature = "async")]
    // TODO: async
    #[cfg(feature = "blocking")]
    nyquest::blocking::backend::register_blocking_backend(crate::blocking::WinrtBlockingBackend);
}
