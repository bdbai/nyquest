use std::ffi::c_char;

use curl::easy::Easy;
use curl_sys::curl_free;

pub fn curl_escape(easy: &Easy, str: impl AsRef<[u8]>) -> Vec<u8> {
    struct CurlString(*mut c_char);
    impl Drop for CurlString {
        fn drop(&mut self) {
            unsafe {
                curl_free(self.0 as _);
            }
        }
    }
    let str = str.as_ref();
    if str.is_empty() {
        return vec![];
    }
    let res_raw = unsafe {
        CurlString(curl_sys::curl_easy_escape(
            easy.raw(),
            str.as_ptr() as _,
            str.len().try_into().expect("str escaped too long"),
        ))
    };
    unsafe { std::ffi::CStr::from_ptr(res_raw.0).to_bytes().to_vec() }
}
