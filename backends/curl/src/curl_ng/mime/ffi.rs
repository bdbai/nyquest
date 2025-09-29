use std::ffi::{c_char, c_int, c_void};

use curl_sys::{
    curl_free_callback, curl_off_t, curl_read_callback, curl_seek_callback, curl_slist, CURLcode,
    CURLoption, CURL, CURLOPTTYPE_OBJECTPOINT,
};
use libc::size_t;

pub const CURLOPT_MIMEPOST: CURLoption = CURLOPTTYPE_OBJECTPOINT + 269;

#[allow(non_camel_case_types)]
pub enum curl_mime {}
#[allow(non_camel_case_types)]
pub enum curl_mimepart {}

extern "C" {
    pub fn curl_mime_init(easy_handle: *mut CURL) -> *mut curl_mime;
    pub fn curl_mime_free(mime_handle: *mut curl_mime);
    pub fn curl_mime_addpart(mime_handle: *mut curl_mime) -> *mut curl_mimepart;
    pub fn curl_mime_data(
        part: *mut curl_mimepart,
        data: *const c_char,
        datasize: size_t,
    ) -> CURLcode;
    pub fn curl_mime_name(part: *mut curl_mimepart, name: *const c_char) -> CURLcode;
    pub fn curl_mime_filename(part: *mut curl_mimepart, filename: *const c_char) -> CURLcode;
    pub fn curl_mime_type(part: *mut curl_mimepart, mimetype: *const c_char) -> CURLcode;
    pub fn curl_mime_data_cb(
        part: *mut curl_mimepart,
        datasize: curl_off_t,
        readfunc: Option<curl_read_callback>,
        seekfunc: Option<curl_seek_callback>,
        freefunc: Option<curl_free_callback>,
        arg: *mut c_void,
    ) -> CURLcode;
    #[allow(dead_code)]
    pub fn curl_mime_subparts(part: *mut curl_mimepart, subparts: *mut curl_mime) -> CURLcode;
    pub fn curl_mime_headers(
        part: *mut curl_mimepart,
        headers: *mut curl_slist,
        take_ownership: c_int,
    ) -> CURLcode;
}
