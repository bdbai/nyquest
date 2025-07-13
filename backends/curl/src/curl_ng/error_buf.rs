use std::ffi::c_char;
use std::marker::PhantomPinned;
use std::mem::MaybeUninit;

use curl_sys::CURL_ERROR_SIZE;

use crate::curl_ng::raw_easy::RawEasy;

pub struct ErrorBuf {
    pub(super) buf: [MaybeUninit<u8>; CURL_ERROR_SIZE],
}

pub struct EasyWithErrorBuf<'e> {
    pub(super) easy: RawEasy,
    pub(super) error_buf: &'e mut ErrorBuf,
}

pub struct OwnedEasyWithErrorBuf {
    pub(super) easy: RawEasy,
    pub(super) error_buf: ErrorBuf,
    __pinned: PhantomPinned,
}

impl ErrorBuf {
    pub fn new() -> Self {
        let mut buf = [MaybeUninit::uninit(); CURL_ERROR_SIZE];
        buf[0].write(0);
        ErrorBuf { buf }
    }
}

impl<'e> EasyWithErrorBuf<'e> {
    pub fn attach(easy: RawEasy, error_buf: &'e mut ErrorBuf) -> Self {
        // SAFETY: The pointer to error buf is guaranteed to be valid for the
        // lifetime of easy handle until it is detached.
        unsafe {
            curl_sys::curl_easy_setopt(
                easy.raw(),
                curl_sys::CURLOPT_ERRORBUFFER,
                error_buf.buf.as_mut_ptr() as *mut c_char,
            );
        }
        EasyWithErrorBuf { easy, error_buf }
    }

    pub fn detach(self) -> RawEasy {
        let null_buf: *mut c_char = std::ptr::null_mut();
        // SAFETY: The `easy` field is valid and the error buffer pointer will
        // become irrelevant after this call.
        unsafe {
            curl_sys::curl_easy_setopt(self.easy.raw(), curl_sys::CURLOPT_ERRORBUFFER, null_buf);
        }
        self.easy
    }
}

impl OwnedEasyWithErrorBuf {
    pub fn new(easy: RawEasy) -> Self {
        let null_buf: *mut c_char = std::ptr::null_mut();
        unsafe {
            curl_sys::curl_easy_setopt(easy.raw(), curl_sys::CURLOPT_ERRORBUFFER, null_buf);
        }
        OwnedEasyWithErrorBuf {
            easy,
            error_buf: ErrorBuf::new(),
            __pinned: PhantomPinned,
        }
    }
}
