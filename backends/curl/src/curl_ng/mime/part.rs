use std::{
    borrow::Cow,
    ffi::{c_char, c_int, c_void},
    io::SeekFrom,
    panic, slice,
};

use curl::easy::{ReadError, SeekResult};
use libc::size_t;

use crate::curl_ng::CurlStringList;

#[derive(Debug)]
pub struct MimePart<R> {
    pub name: Cow<'static, str>,
    pub filename: Option<Cow<'static, str>>,
    pub content_type: Option<Cow<'static, str>>,
    pub header_list: Option<CurlStringList>,
    pub content: MimePartContent<R>,
}

#[derive(Debug)]
pub enum MimePartContent<R> {
    Data(Cow<'static, [u8]>),
    Reader { reader: R, size: Option<i64> },
}

pub trait MimePartReader {
    fn read(&mut self, data: &mut [u8]) -> Result<usize, ReadError>;
    fn seek(&mut self, whence: SeekFrom) -> SeekResult;
}

pub(super) extern "C" fn read_cb<P: MimePartReader + Send + 'static>(
    ptr: *mut c_char,
    size: size_t,
    nmemb: size_t,
    data: *mut c_void,
) -> size_t {
    panic::catch_unwind(|| unsafe {
        let input = slice::from_raw_parts_mut(ptr as *mut u8, size * nmemb);
        match (*(data as *mut P)).read(input) {
            Ok(s) => s,
            Err(ReadError::Pause) => curl_sys::CURL_READFUNC_PAUSE,
            Err(ReadError::Abort) => curl_sys::CURL_READFUNC_ABORT,
            Err(_) => curl_sys::CURL_READFUNC_ABORT,
        }
    })
    .unwrap_or(!0)
}

pub(super) extern "C" fn seek_cb<P: MimePartReader + Send + 'static>(
    data: *mut c_void,
    offset: curl_sys::curl_off_t,
    origin: c_int,
) -> c_int {
    panic::catch_unwind(|| unsafe {
        let from = if origin == libc::SEEK_SET {
            SeekFrom::Start(offset as u64)
        } else {
            panic!("unknown origin from libcurl: {}", origin);
        };
        (*(data as *mut P)).seek(from) as c_int
    })
    .unwrap_or(!0)
}

pub(super) extern "C" fn free_cb<P: MimePartReader + Send + 'static>(data: *mut c_void) {
    panic::catch_unwind(|| unsafe {
        let _ = Box::from_raw(data as *mut P);
    })
    .unwrap_or(());
}
