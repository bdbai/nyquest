use std::{ffi::c_void, io::SeekFrom, marker::PhantomPinned, panic, pin::Pin};

use curl::easy::{ReadError, SeekResult, WriteError};
use pin_project_lite::pin_project;

use crate::curl_ng::{easy_ref::setopt_ptr, error_context::WithCurlCodeContext, raw_easy::RawEasy};

use super::{easy_ref::AsRawEasyMut, error_context::CurlCodeContext};

pub trait EasyCallback {
    fn write(self: Pin<&mut Self>, data: &[u8]) -> Result<usize, WriteError>;
    fn read(self: Pin<&mut Self>, buf: &mut [u8]) -> Result<usize, ReadError>;
    fn header(self: Pin<&mut Self>, data: &[u8]) -> bool;
    fn seek(self: Pin<&mut Self>, offset: i64, whence: SeekFrom) -> Result<i64, SeekResult>;
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
}

impl<E: AsRawEasyMut, C: EasyCallback> AsRawEasyMut for EasyWithCallback<E, C> {
    fn init(mut self: Pin<&mut Self>) -> Result<(), CurlCodeContext> {
        let self_ptr: *mut Self = unsafe { self.as_mut().get_unchecked_mut() };
        let mut this = self.project();
        this.easy.as_mut().init()?;
        let raw = this.easy.as_raw_easy_mut().raw();
        setopt_ptr(raw, curl_sys::CURLOPT_WRITEDATA, self_ptr as _)
            .with_easy_context("setopt WRITEDATA")?;
        setopt_ptr(
            raw,
            curl_sys::CURLOPT_WRITEFUNCTION,
            write_callback::<E, C> as _,
        )
        .with_easy_context("setopt WRITEFUNCTION")?;
        todo!("Implement read, seek, and header callbacks");
    }

    fn as_raw_easy_mut(self: Pin<&mut Self>) -> Pin<&mut RawEasy> {
        let this = self.project();
        this.easy.as_raw_easy_mut()
    }

    fn reset_extra(self: Pin<&mut Self>) -> Result<(), CurlCodeContext> {
        self.project().easy.reset_extra()
    }
}

fn write_callback<E: AsRawEasyMut, C: EasyCallback>(
    data: *mut u8,
    size: usize,
    nmemb: usize,
    userdata: *mut c_void,
) -> usize {
    panic::catch_unwind(|| {
        let this = unsafe { Pin::new_unchecked(&mut *(userdata as *mut EasyWithCallback<E, C>)) };
        let data = unsafe { std::slice::from_raw_parts(data, size * nmemb) };
        let res = this.project().callback.write(data);
        match res {
            Ok(n) => n,
            Err(_) => curl_sys::CURL_WRITEFUNC_PAUSE,
        }
    })
    .unwrap_or(!0)
}

// TODO: read_callback, seek_callback, header_callback
