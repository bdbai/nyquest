use curl_sys::{CURLPAUSE_RECV, CURLPAUSE_SEND};

pub const _CURLPAUSE_ALL: i32 = CURLPAUSE_RECV | CURLPAUSE_SEND;

#[derive(Clone, Copy)]
pub(super) struct EasyPause(*mut curl_sys::CURL);

impl EasyPause {
    pub(super) fn new(handle: *mut curl_sys::CURL) -> Self {
        Self(handle)
    }

    /// ## Safety
    /// The caller must ensure:
    /// 1. The handle is a valid CURL handle.
    /// 2. The handle is either within the same thread or we are in a callback.
    pub(super) unsafe fn pause_recv(&self) {
        curl_sys::curl_easy_pause(self.0, CURLPAUSE_RECV);
    }
}

// Safety: Nothing can happen when the handle is moved between threads without "unsafe"
unsafe impl Send for EasyPause {}
