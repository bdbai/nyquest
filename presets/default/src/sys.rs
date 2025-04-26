use cfg_if::cfg_if;

cfg_if! {
    if #[cfg(windows)] {
        pub use nyquest_backend_winrt::WinrtBackend as Backend;
        pub fn register() {
            nyquest_backend_winrt::register();
        }
    } else if #[cfg(target_vendor = "apple")] {
        pub use nyquest_backend_nsurlsession::NSUrlSessionBackend as Backend;
        pub fn register() {
            nyquest_backend_nsurlsession::register();
        }
    } else {
        pub use nyquest_backend_curl::CurlBackend as Backend;
        pub fn register() {
            nyquest_backend_curl::register();
        }
    }
}
