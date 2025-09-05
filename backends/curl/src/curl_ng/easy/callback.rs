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
    easy::{setopt_ptr, AsRawEasyMut, RawEasy},
    CurlCodeContext, WithCurlCodeContext as _,
};

pub trait EasyCallback {
    fn write(self: Pin<&mut Self>, data: &[u8]) -> Result<usize, WriteError>;
    fn read(self: Pin<&mut Self>, buf: &mut [u8]) -> Result<usize, ReadError>;
    fn header(self: Pin<&mut Self>, data: &[u8]) -> bool;
    fn seek(self: Pin<&mut Self>, offset: i64, whence: SeekFrom) -> SeekResult;
}

pin_project! {
    pub struct EasyWithCallback<E, C> {
        #[pin]
        easy: E,
        #[pin]
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

    pub fn as_easy_mut(self: Pin<&mut Self>) -> Pin<&mut E> {
        self.project().easy
    }
    pub fn as_callback_mut(self: Pin<&mut Self>) -> Pin<&mut C> {
        self.project().callback
    }
}

impl<E: AsRawEasyMut, C: EasyCallback> EasyWithCallback<E, C> {
    fn bind_callbacks(mut self: Pin<&mut Self>) -> Result<(), CurlCodeContext> {
        let this = self.as_mut().project();
        let raw = this.easy.as_raw_easy_mut().raw();
        unsafe {
            let self_ptr: *mut Self = self.get_unchecked_mut();
            setopt_ptr(raw, curl_sys::CURLOPT_WRITEDATA, self_ptr as _)
                .with_easy_context("setopt WRITEDATA")?;
            setopt_ptr(
                raw,
                curl_sys::CURLOPT_WRITEFUNCTION,
                write_callback::<E, C> as _,
            )
            .with_easy_context("setopt WRITEFUNCTION")?;
            setopt_ptr(raw, curl_sys::CURLOPT_READDATA, self_ptr as _)
                .with_easy_context("setopt READDATA")?;
            setopt_ptr(
                raw,
                curl_sys::CURLOPT_READFUNCTION,
                read_callback::<E, C> as _,
            )
            .with_easy_context("setopt READFUNCTION")?;
            setopt_ptr(raw, curl_sys::CURLOPT_HEADERDATA, self_ptr as _)
                .with_easy_context("setopt HEADERDATA")?;
            setopt_ptr(
                raw,
                curl_sys::CURLOPT_HEADERFUNCTION,
                header_callback::<E, C> as _,
            )
            .with_easy_context("setopt HEADERFUNCTION")?;
            setopt_ptr(raw, curl_sys::CURLOPT_SEEKDATA, self_ptr as _)
                .with_easy_context("setopt SEEKDATA")?;
            setopt_ptr(
                raw,
                curl_sys::CURLOPT_SEEKFUNCTION,
                seek_callback::<E, C> as _,
            )
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
        this.project().callback.seek(offset, whence) as c_int
    })
    .unwrap_or(curl_sys::CURL_SEEKFUNC_FAIL)
}
