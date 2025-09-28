use std::{
    ffi::{c_char, c_int, c_void},
    io::SeekFrom,
    marker::PhantomPinned,
    panic,
    pin::Pin,
};

use curl::easy::{ReadError, SeekResult, WriteError};
use pin_project_lite::pin_project;

use crate::curl_ng::{
    easy::{AsRawEasyMut, RawEasy},
    CurlCodeContext, WithCurlCodeContext as _,
};

// TODO: pass easy handle to callback methods for pause etc.
pub trait EasyCallback {
    fn write(&mut self, data: &[u8]) -> Result<usize, WriteError>;
    fn read(&mut self, buf: &mut [u8]) -> Result<usize, ReadError>;
    fn header(&mut self, data: &[u8]) -> bool;
    fn seek(&mut self, whence: SeekFrom) -> SeekResult;
}

pin_project! {
    pub struct EasyWithCallback<E, C> {
        #[pin]
        easy: E,
        callback: C,
        __pinned: PhantomPinned,
    }
}

impl<E, C> EasyWithCallback<E, C> {
    pub fn new(easy: E, callback: C) -> Self {
        EasyWithCallback {
            easy,
            callback,
            __pinned: PhantomPinned,
        }
    }

    pub fn _as_easy_mut(self: Pin<&mut Self>) -> Pin<&mut E> {
        self.project().easy
    }
    pub fn as_callback_mut(self: Pin<&mut Self>) -> &mut C {
        self.project().callback
    }
}

impl<E: AsRawEasyMut, C: EasyCallback> EasyWithCallback<E, C> {
    fn bind_callbacks(mut self: Pin<&mut Self>) -> Result<(), CurlCodeContext> {
        let self_ptr: *mut Self = unsafe { self.as_mut().get_unchecked_mut() };
        let this = self.as_mut().project();
        let mut raw = this.easy.as_raw_easy_mut();
        unsafe {
            raw.as_mut()
                .setopt_ptr(curl_sys::CURLOPT_WRITEDATA, self_ptr as _)
                .with_easy_context("setopt WRITEDATA")?;
            raw.as_mut()
                .setopt_ptr(curl_sys::CURLOPT_WRITEFUNCTION, write_callback::<E, C> as _)
                .with_easy_context("setopt WRITEFUNCTION")?;
            raw.as_mut()
                .setopt_ptr(curl_sys::CURLOPT_READDATA, self_ptr as _)
                .with_easy_context("setopt READDATA")?;
            raw.as_mut()
                .setopt_ptr(curl_sys::CURLOPT_READFUNCTION, read_callback::<E, C> as _)
                .with_easy_context("setopt READFUNCTION")?;
            raw.as_mut()
                .setopt_ptr(curl_sys::CURLOPT_HEADERDATA, self_ptr as _)
                .with_easy_context("setopt HEADERDATA")?;
            raw.as_mut()
                .setopt_ptr(
                    curl_sys::CURLOPT_HEADERFUNCTION,
                    header_callback::<E, C> as _,
                )
                .with_easy_context("setopt HEADERFUNCTION")?;
            raw.as_mut()
                .setopt_ptr(curl_sys::CURLOPT_SEEKDATA, self_ptr as _)
                .with_easy_context("setopt SEEKDATA")?;
            raw.as_mut()
                .setopt_ptr(curl_sys::CURLOPT_SEEKFUNCTION, seek_callback::<E, C> as _)
                .with_easy_context("setopt SEEKFUNCTION")?;
        }
        Ok(())
    }
}

impl<E: AsRawEasyMut, C: EasyCallback> AsRawEasyMut for EasyWithCallback<E, C> {
    fn init(mut self: Pin<&mut Self>) -> Result<(), CurlCodeContext> {
        let mut this = self.as_mut().project();
        this.easy.as_mut().init()?;
        self.bind_callbacks()
    }

    fn as_raw_easy_mut(self: Pin<&mut Self>) -> Pin<&mut RawEasy> {
        let this = self.project();
        this.easy.as_raw_easy_mut()
    }

    fn reset(mut self: Pin<&mut Self>) -> Result<(), CurlCodeContext> {
        self.as_mut().project().easy.reset()?;
        self.bind_callbacks()
    }
}

fn write_callback<E: AsRawEasyMut, C: EasyCallback>(
    data: *mut c_char,
    size: usize,
    nmemb: usize,
    userdata: *mut c_void,
) -> usize {
    panic::catch_unwind(|| {
        let this = unsafe { Pin::new_unchecked(&mut *(userdata as *mut EasyWithCallback<E, C>)) };
        let data = unsafe { std::slice::from_raw_parts(data as *mut u8, size * nmemb) };
        let res = this.project().callback.write(data);
        match res {
            Ok(n) => n,
            Err(_) => curl_sys::CURL_WRITEFUNC_PAUSE,
        }
    })
    .unwrap_or(!0)
}

fn read_callback<E: AsRawEasyMut, C: EasyCallback>(
    buffer: *mut c_char,
    size: usize,
    nitems: usize,
    userdata: *mut c_void,
) -> usize {
    panic::catch_unwind(|| {
        let this = unsafe { Pin::new_unchecked(&mut *(userdata as *mut EasyWithCallback<E, C>)) };
        let buffer = unsafe { std::slice::from_raw_parts_mut(buffer as *mut u8, size * nitems) };
        let res = this.project().callback.read(buffer);
        match res {
            Ok(n) => n,
            Err(ReadError::Pause) => curl_sys::CURL_READFUNC_PAUSE,
            Err(_) => curl_sys::CURL_READFUNC_ABORT,
        }
    })
    .unwrap_or(!0)
}

fn header_callback<E: AsRawEasyMut, C: EasyCallback>(
    buffer: *mut c_char,
    size: usize,
    nitems: usize,
    userdata: *mut c_void,
) -> usize {
    panic::catch_unwind(|| {
        let this = unsafe { Pin::new_unchecked(&mut *(userdata as *mut EasyWithCallback<E, C>)) };
        let data = unsafe { std::slice::from_raw_parts(buffer as *mut u8, size * nitems) };
        let res = this.project().callback.header(data);
        if res {
            size * nitems
        } else {
            !0
        }
    })
    .unwrap_or(!0)
}

fn seek_callback<E: AsRawEasyMut, C: EasyCallback>(
    userdata: *mut c_void,
    offset: curl_sys::curl_off_t,
    origin: c_int,
) -> c_int {
    panic::catch_unwind(|| {
        let this = unsafe { Pin::new_unchecked(&mut *(userdata as *mut EasyWithCallback<E, C>)) };
        let whence = match origin {
            libc::SEEK_SET => SeekFrom::Start(offset as u64),
            libc::SEEK_CUR => SeekFrom::Current(offset),
            libc::SEEK_END => SeekFrom::End(offset),
            _ => return curl_sys::CURL_SEEKFUNC_FAIL,
        };
        this.project().callback.seek(whence) as c_int
    })
    .unwrap_or(curl_sys::CURL_SEEKFUNC_FAIL)
}
