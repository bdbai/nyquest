use std::{
    borrow::Cow,
    ffi::{c_char, c_long},
    pin::Pin,
    ptr::null,
};

use crate::curl_ng::{
    easy::{AsRawEasyMut as _, RawEasy},
    ffi::transform_cow_str_to_c_str,
    CurlCodeContext, WithCurlCodeContext as _,
};

impl RawEasy {
    pub(super) unsafe fn setopt_str(
        self: Pin<&mut Self>,
        opt: curl_sys::CURLoption,
        mut val: Cow<'_, str>,
    ) -> curl_sys::CURLcode {
        self.setopt_ptr(opt, transform_cow_str_to_c_str(&mut val))
    }

    pub(super) unsafe fn setopt_ptr(
        self: Pin<&mut Self>,
        opt: curl_sys::CURLoption,
        val: *const c_char,
    ) -> curl_sys::CURLcode {
        unsafe { curl_sys::curl_easy_setopt(self.as_raw_easy_mut().raw(), opt, val) }
    }

    pub(super) unsafe fn setopt_long(
        self: Pin<&mut Self>,
        opt: curl_sys::CURLoption,
        val: libc::c_long,
    ) -> curl_sys::CURLcode {
        unsafe { curl_sys::curl_easy_setopt(self.as_raw_easy_mut().raw(), opt, val) }
    }

    pub(super) unsafe fn setopt_off_t(
        self: Pin<&mut Self>,
        opt: curl_sys::CURLoption,
        val: curl_sys::curl_off_t,
    ) -> curl_sys::CURLcode {
        unsafe { curl_sys::curl_easy_setopt(self.as_raw_easy_mut().raw(), opt, val) }
    }

    pub fn set_nosignal(self: Pin<&mut Self>, no_signal: bool) -> Result<(), CurlCodeContext> {
        unsafe {
            self.setopt_long(curl_sys::CURLOPT_NOSIGNAL, no_signal as c_long)
                .with_easy_context("setopt CURLOPT_NOSIGNAL")
        }
    }

    pub fn set_noproxy<'s>(
        self: Pin<&mut Self>,
        skip: impl Into<Cow<'s, str>>,
    ) -> Result<(), CurlCodeContext> {
        unsafe {
            self.setopt_str(curl_sys::CURLOPT_NOPROXY, skip.into())
                .with_easy_context("setopt CURLOPT_NOPROXY")
        }
    }

    pub fn set_useragent<'s>(
        self: Pin<&mut Self>,
        user_agent: impl Into<Cow<'s, str>>,
    ) -> Result<(), CurlCodeContext> {
        unsafe {
            self.setopt_str(curl_sys::CURLOPT_USERAGENT, user_agent.into())
                .with_easy_context("setopt CURLOPT_USERAGENT")
        }
    }

    pub fn set_cookiefile<'s>(
        self: Pin<&mut Self>,
        file: impl Into<Cow<'s, str>>,
    ) -> Result<(), CurlCodeContext> {
        unsafe {
            self.setopt_str(curl_sys::CURLOPT_COOKIEFILE, file.into())
                .with_easy_context("setopt CURLOPT_COOKIEFILE")
        }
    }

    pub fn set_timeout(
        self: Pin<&mut Self>,
        timeout: std::time::Duration,
    ) -> Result<(), CurlCodeContext> {
        unsafe {
            let ms = timeout.as_millis();
            match c_long::try_from(ms) {
                Ok(amt) => self.setopt_long(curl_sys::CURLOPT_TIMEOUT_MS, amt),
                Err(_) => {
                    let amt = c_long::try_from(ms / 1000).map_err(|_| {
                        curl_sys::CURLE_BAD_FUNCTION_ARGUMENT
                            .with_easy_context("setopt CURLOPT_TIMEOUT convert")
                            .unwrap_err()
                    })?;
                    self.setopt_long(curl_sys::CURLOPT_TIMEOUT, amt)
                }
            }
            .with_easy_context("setopt CURLOPT_TIMEOUT(_MS)")
        }
    }

    pub fn set_ssl_verify_peer(self: Pin<&mut Self>, verify: bool) -> Result<(), CurlCodeContext> {
        unsafe {
            self.setopt_long(curl_sys::CURLOPT_SSL_VERIFYPEER, verify as c_long)
                .with_easy_context("setopt CURLOPT_SSL_VERIFYPEER")
        }
    }

    pub fn _set_ssl_verify_host(self: Pin<&mut Self>, verify: bool) -> Result<(), CurlCodeContext> {
        unsafe {
            self.setopt_long(curl_sys::CURLOPT_SSL_VERIFYHOST, verify as c_long)
                .with_easy_context("setopt CURLOPT_SSL_VERIFYHOST")
        }
    }

    pub fn set_follow_location(self: Pin<&mut Self>, follow: bool) -> Result<(), CurlCodeContext> {
        unsafe {
            self.setopt_long(curl_sys::CURLOPT_FOLLOWLOCATION, follow as c_long)
                .with_easy_context("setopt CURLOPT_FOLLOWLOCATION")
        }
    }

    pub fn set_url<'s>(
        self: Pin<&mut Self>,
        url: impl Into<Cow<'s, str>>,
    ) -> Result<(), CurlCodeContext> {
        unsafe {
            self.setopt_str(curl_sys::CURLOPT_URL, url.into())
                .with_easy_context("setopt CURLOPT_URL")
        }
    }

    pub fn set_get(self: Pin<&mut Self>, get: bool) -> Result<(), CurlCodeContext> {
        unsafe {
            self.setopt_long(curl_sys::CURLOPT_HTTPGET, get as c_long)
                .with_easy_context("setopt CURLOPT_HTTPGET")
        }
    }

    pub fn set_post(self: Pin<&mut Self>, post: bool) -> Result<(), CurlCodeContext> {
        unsafe {
            self.setopt_long(curl_sys::CURLOPT_POST, post as c_long)
                .with_easy_context("setopt CURLOPT_POST")
        }
    }

    pub fn set_upload(self: Pin<&mut Self>, put: bool) -> Result<(), CurlCodeContext> {
        unsafe {
            self.setopt_long(curl_sys::CURLOPT_UPLOAD, put as c_long)
                .with_easy_context("setopt CURLOPT_UPLOAD")
        }
    }

    pub fn set_nobody(self: Pin<&mut Self>, nobody: bool) -> Result<(), CurlCodeContext> {
        unsafe {
            self.setopt_long(curl_sys::CURLOPT_NOBODY, nobody as c_long)
                .with_easy_context("setopt CURLOPT_NOBODY")
        }
    }

    pub fn set_custom_request<'s>(
        self: Pin<&mut Self>,
        method: impl Into<Cow<'s, str>>,
    ) -> Result<(), CurlCodeContext> {
        unsafe {
            self.setopt_str(curl_sys::CURLOPT_CUSTOMREQUEST, method.into())
                .with_easy_context("setopt CURLOPT_CUSTOMREQUEST")
        }
    }

    pub fn set_infile_size(self: Pin<&mut Self>, size: u64) -> Result<(), CurlCodeContext> {
        unsafe {
            self.setopt_off_t(
                curl_sys::CURLOPT_INFILESIZE_LARGE,
                size as curl_sys::curl_off_t,
            )
            .with_easy_context("setopt CURLOPT_INFILESIZE_LARGE")
        }
    }

    pub fn set_post_fields_copy(
        mut self: Pin<&mut Self>,
        data: Option<&[u8]>,
    ) -> Result<(), CurlCodeContext> {
        unsafe {
            self.as_mut()
                .setopt_ptr(curl_sys::CURLOPT_POSTFIELDS, null())
                .with_easy_context("setopt CURLOPT_POSTFIELDS null")?;
            if let Some(data) = data {
                self.as_mut()
                    .setopt_off_t(
                        curl_sys::CURLOPT_POSTFIELDSIZE_LARGE,
                        data.len() as curl_sys::curl_off_t,
                    )
                    .with_easy_context("setopt CURLOPT_POSTFIELDSIZE_LARGE")?;
                self.setopt_ptr(curl_sys::CURLOPT_COPYPOSTFIELDS, data.as_ptr() as _)
                    .with_easy_context("setopt CURLOPT_POSTFIELDS")?;
            }
        }
        Ok(())
    }

    pub fn set_accept_encoding<'s>(
        self: Pin<&mut Self>,
        enc: impl Into<Cow<'s, str>>,
    ) -> Result<(), CurlCodeContext> {
        unsafe {
            self.setopt_str(curl_sys::CURLOPT_ACCEPT_ENCODING, enc.into())
                .with_easy_context("setopt CURLOPT_ACCEPT_ENCODING")
        }
    }
}
