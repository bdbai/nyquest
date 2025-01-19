#[cfg(feature = "async")]
pub mod r#async;
#[cfg(feature = "blocking")]
pub mod blocking;
mod buffer;
mod client;
mod error;
mod request;
mod response;
mod string_pair;
mod uri;

#[derive(Clone)]
pub struct WinrtBackend;

#[cfg(windows)]
pub fn register() {
    nyquest::register_backend(WinrtBackend);
}
