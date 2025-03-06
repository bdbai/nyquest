cfg_if::cfg_if! {
    if #[cfg(target_vendor = "apple")] {
        #[cfg(feature = "blocking")]
        pub mod blocking;
        #[cfg(feature = "async")]
        pub mod r#async;
    }
}

#[derive(Clone)]
pub struct NSUrlSessionBackend;

#[cfg(target_vendor = "apple")]
pub fn register() {
    nyquest_interface::register_backend(NSUrlSessionBackend);
}
