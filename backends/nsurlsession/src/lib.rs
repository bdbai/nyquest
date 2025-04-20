cfg_if::cfg_if! {
    if #[cfg(target_vendor = "apple")] {
        #[cfg(feature = "blocking")]
        pub mod blocking;
        #[cfg(feature = "async")]
        pub mod r#async;
    }
}

mod client;
mod datatask;
mod error;
mod response;

#[derive(Clone)]
pub struct NSUrlSessionBackend;

#[cfg(target_vendor = "apple")]
pub fn register() {
    nyquest_interface::register_backend(NSUrlSessionBackend);
}
