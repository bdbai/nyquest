#[cfg(windows)]
pub fn register() {
    nyquest_backend_winrt::register();
}

#[cfg(target_vendor = "apple")]
pub fn register() {
    nyquest_backend_nsurlsession::register();
}

#[cfg(not(any(windows, target_vendor = "apple")))]
pub fn register() {
    nyquest_backend_curl::register();
}
