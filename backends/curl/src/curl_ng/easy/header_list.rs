use std::{pin::Pin, ptr::null};

use pin_project_lite::pin_project;

use crate::curl_ng::{
    easy::{AsRawEasyMut, RawEasy},
    error_context::{CurlCodeContext, WithCurlCodeContext as _},
    CurlStringList,
};

pin_project! {
    pub struct EasyWithHeaderList<E> {
        #[pin]
        easy: E,
        list: Option<CurlStringList>,
    }
}

impl<E> EasyWithHeaderList<E> {
    pub fn new(easy: E) -> Self {
        EasyWithHeaderList { easy, list: None }
    }

    pub fn as_easy_mut(self: Pin<&mut Self>) -> Pin<&mut E> {
        self.project().easy
    }
}

impl<E: AsRawEasyMut> EasyWithHeaderList<E> {
    pub fn set_headers(
        mut self: Pin<&mut Self>,
        headers: Option<CurlStringList>,
    ) -> Result<(), CurlCodeContext> {
        let this = self.as_mut().project();
        let raw = this.easy.as_raw_easy_mut();
        let raw_list = headers.as_ref().map_or(null(), |l| l.raw());
        unsafe {
            raw.setopt_ptr(curl_sys::CURLOPT_HTTPHEADER, raw_list as *const _)
                .with_easy_context("setopt CURLOPT_HTTPHEADER")?
        }
        *this.list = headers;
        Ok(())
    }
}

impl<E: AsRawEasyMut> AsRawEasyMut for EasyWithHeaderList<E> {
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
