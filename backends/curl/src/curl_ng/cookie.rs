use std::ffi::CStr;
use std::pin::Pin;
use std::ptr::null_mut;

use crate::curl_ng::{
    easy::{RawEasy, Share},
    CurlCodeContext, CurlStringList, WithCurlCodeContext,
};

impl RawEasy {
    /// Enables the cookie engine without loading cookies from a file.
    pub fn enable_cookie_engine(self: Pin<&mut Self>) -> Result<(), CurlCodeContext> {
        self.set_cookiefile("")
    }

    /// Adds a cookie to the cookie store, in `Set-Cookie` line format without attributes.
    pub fn set_cookielist(self: Pin<&mut Self>, cookie: &CStr) -> Result<(), CurlCodeContext> {
        unsafe { self.setopt_ptr(curl_sys::CURLOPT_COOKIELIST, cookie.as_ptr()) }
            .with_easy_context("setopt CURLOPT_COOKIELIST")
    }

    /// Returns the cookies libcurl holds, in Netscape file format.
    pub fn get_cookielist(&self) -> Result<CurlStringList, CurlCodeContext> {
        let mut list: *mut curl_sys::curl_slist = null_mut();
        unsafe {
            curl_sys::curl_easy_getinfo(self.raw(), curl_sys::CURLINFO_COOKIELIST, &mut list)
        }
        .with_easy_context("getinfo CURLINFO_COOKIELIST")?;
        Ok(CurlStringList::from_raw(list))
    }
}

/// Runs `f` on a temporary easy handle sharing cookies with `share`.
pub(crate) fn with_share_easy<T>(
    share: &Share,
    f: impl FnOnce(Pin<&mut RawEasy>) -> Result<T, CurlCodeContext>,
) -> Result<T, CurlCodeContext> {
    let mut easy = RawEasy::new();
    let mut easy = Pin::new(&mut easy);
    unsafe {
        easy.as_mut()
            .setopt_ptr(curl_sys::CURLOPT_SHARE, share.as_raw() as _)
    }
    .with_easy_context("setopt CURLOPT_SHARE")?;
    easy.as_mut().enable_cookie_engine()?;
    f(easy)
}
