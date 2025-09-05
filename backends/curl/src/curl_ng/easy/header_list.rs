use std::{mem::transmute_copy, pin::Pin};

use curl::easy::List;
use pin_project_lite::pin_project;

use crate::curl_ng::{
    easy::{setopt_ptr, AsRawEasyMut, RawEasy},
    error_context::{CurlCodeContext, WithCurlCodeContext as _},
};

pin_project! {
    pub struct HeaderList<E> {
        #[pin]
        easy: E,
        list: Option<List>,
    }
}

impl<E> HeaderList<E> {
    pub fn new(easy: E) -> Self {
        HeaderList { easy, list: None }
    }
}

impl<E: AsRawEasyMut> HeaderList<E> {
    pub fn set_headers(
        mut self: Pin<&mut Self>,
        headers: Option<List>,
    ) -> Result<(), CurlCodeContext> {
        let this = self.as_mut().project();
        let raw = this.easy.as_raw_easy_mut().raw();
        let raw_list = this.list.as_ref().map_or(std::ptr::null(), |l| {
            // TODO: rewrite our own List wrapper
            assert_eq!(
                size_of::<List>(),
                size_of::<*mut curl_sys::curl_slist>(),
                "List size is not equal to curl_slist pointer size"
            );
            unsafe { transmute_copy::<List, *mut curl_sys::curl_slist>(l) }
        });
        unsafe {
            setopt_ptr(raw, curl_sys::CURLOPT_HTTPHEADER, raw_list as *const _)
                .with_easy_context("setopt CURLOPT_HTTPHEADER")?
        }
        *this.list = headers;
        Ok(())
    }
}

impl<E: AsRawEasyMut> AsRawEasyMut for HeaderList<E> {
    fn init(self: Pin<&mut Self>) -> Result<(), CurlCodeContext> {
        self.project().easy.init()
    }

    fn as_raw_easy_mut(self: Pin<&mut Self>) -> Pin<&mut RawEasy> {
        self.project().easy.as_raw_easy_mut()
    }

    fn reset(mut self: Pin<&mut Self>) -> Result<(), CurlCodeContext> {
        let this = self.as_mut().project();
        this.easy.reset()?;
        self.set_headers(None)
    }
}
