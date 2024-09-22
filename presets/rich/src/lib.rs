#[cfg(windows)]
pub fn register() {
    nyquest_backend_winrt::register();
}

#[cfg(not(windows))]
pub fn register() {
    nyquest_backend_curl::register();
}
