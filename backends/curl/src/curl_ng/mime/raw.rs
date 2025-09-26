use crate::curl_ng::mime::ffi::{curl_mime, curl_mime_free};

#[derive(Debug)]
pub(crate) struct RawMime(pub(crate) *mut curl_mime);

unsafe impl Send for RawMime {}

impl Drop for RawMime {
    fn drop(&mut self) {
        unsafe { curl_mime_free(self.0) }
    }
}
