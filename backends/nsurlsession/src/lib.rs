#[cfg(target_vendor = "apple")]
pub mod blocking;

#[derive(Clone)]
pub struct NSUrlSessionBackend;

#[cfg(target_vendor = "apple")]
pub fn register() {
    nyquest_interface::register_backend(NSUrlSessionBackend);
}
