cfg_if::cfg_if! {
    if #[cfg(windows)] {
        #[cfg(feature = "async")]
        pub mod r#async;
        #[cfg(feature = "blocking")]
        pub mod blocking;
        mod buffer;
        mod client;
        mod error;
        mod ibuffer;
        mod request;
        mod response;
        mod response_size_limiter;
        mod string_pair;
        mod uri;
    }
}

#[derive(Clone)]
pub struct WinrtBackend;

#[cfg(windows)]
pub fn register() {
    nyquest_interface::register_backend(WinrtBackend);
}
