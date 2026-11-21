use std::ffi::{c_char, c_int, CStr};
use std::ptr::{null_mut, NonNull};

use crate::curl_ng::{CurlCodeContext, WithCurlCodeContext};

const CURLUPART_URL: c_int = 0;
#[cfg(feature = "cookies")]
const CURLUPART_SCHEME: c_int = 1;
#[cfg(feature = "cookies")]
const CURLUPART_HOST: c_int = 5;
#[cfg(feature = "cookies")]
const CURLUPART_PATH: c_int = 7;
#[cfg(feature = "cookies")]
const CURLU_DEFAULT_SCHEME: u32 = 1 << 2;
const CURLU_NON_SUPPORT_SCHEME: u32 = 1 << 3;
#[cfg(feature = "cookies")]
const CURLU_PUNYCODE: u32 = 1 << 12;

#[repr(C)]
struct CurlUrlHandle {
    _private: [u8; 0],
}

extern "C" {
    fn curl_url() -> *mut CurlUrlHandle;
    fn curl_url_cleanup(handle: *mut CurlUrlHandle);
    fn curl_url_dup(handle: *const CurlUrlHandle) -> *mut CurlUrlHandle;
    fn curl_url_set(
        handle: *mut CurlUrlHandle,
        part: c_int,
        content: *const c_char,
        flags: u32,
    ) -> c_int;
    fn curl_url_get(
        handle: *const CurlUrlHandle,
        part: c_int,
        content: *mut *mut c_char,
        flags: u32,
    ) -> c_int;
}

/// A `CURLU` handle in libcurl, for parsing URLs.
pub struct CurlUrl(NonNull<CurlUrlHandle>);

unsafe impl Send for CurlUrl {}
// The base handle is never mutated through `&self`: all mutation goes through
// `&mut self` on an owned (hence exclusive) handle, while `dup`/`get` only
// read. This makes sharing an immutable base across threads sound.
unsafe impl Sync for CurlUrl {}

impl Clone for CurlUrl {
    fn clone(&self) -> Self {
        self.try_clone()
            .expect("failed to duplicate a libcurl URL handle")
    }
}

impl Drop for CurlUrl {
    fn drop(&mut self) {
        unsafe { curl_url_cleanup(self.0.as_ptr()) };
    }
}

impl CurlUrl {
    /// Parses `url`, applying a default scheme when it has none.
    #[cfg(feature = "cookies")]
    pub fn parse(url: &CStr) -> Result<Self, CurlCodeContext> {
        Self::parse_with_flags(url, CURLU_DEFAULT_SCHEME)
    }

    fn parse_with_flags(url: &CStr, flags: u32) -> Result<Self, CurlCodeContext> {
        let handle = unsafe { curl_url() };
        let handle = NonNull::new(handle).expect("failed to allocate a libcurl URL handle");
        let handle = Self(handle);
        unsafe { curl_url_set(handle.0.as_ptr(), CURLUPART_URL, url.as_ptr(), flags) }
            .with_easy_context("curl_url_set CURLUPART_URL")?;
        Ok(handle)
    }

    /// Parses a `base_url` for later per-request resolution.
    ///
    /// Unlike [`CurlUrl::parse`], no default scheme is applied so that a
    /// schemeless base fails fast instead of silently becoming `https://`.
    /// `CURLU_NON_SUPPORT_SCHEME` is set so that `ws://`/`wss://` (and other
    /// schemes libcurl has no protocol support for) are accepted.
    pub fn parse_base(url: &CStr) -> Result<Self, CurlCodeContext> {
        Self::parse_with_flags(url, CURLU_NON_SUPPORT_SCHEME)
    }

    /// Parses a standalone request URL (no base) for normalization.
    pub fn parse_request(url: &CStr) -> Result<Self, CurlCodeContext> {
        Self::parse_with_flags(url, CURLU_NON_SUPPORT_SCHEME)
    }

    fn try_clone(&self) -> Result<Self, CurlCodeContext> {
        let handle = unsafe { curl_url_dup(self.0.as_ptr()) };
        NonNull::new(handle).map(Self).ok_or_else(|| {
            curl_sys::CURLE_OUT_OF_MEMORY
                .with_easy_context("curl_url_dup")
                .unwrap_err()
        })
    }

    /// Resolves `relative` against `self` following RFC 3986, returning the
    /// resulting absolute URL.
    ///
    /// When `relative` is absolute, it replaces the base (per `curl_url_set`
    /// semantics for `CURLUPART_URL` on a populated handle).
    pub fn resolve(&self, relative: &CStr) -> Result<String, CurlCodeContext> {
        let mut duped = self.try_clone()?;
        duped.set_relative(relative)?;
        duped.get_url()
    }

    /// Resolves `relative` against a base handle, or parses it standalone
    /// when `base` is `None`.
    pub fn join(base: Option<&Self>, relative: &CStr) -> Result<String, CurlCodeContext> {
        match base {
            Some(base) => base.resolve(relative),
            None => Ok(Self::parse_request(relative)?.get_url()?),
        }
    }

    fn set_relative(&mut self, relative: &CStr) -> Result<(), CurlCodeContext> {
        unsafe {
            curl_url_set(
                self.0.as_ptr(),
                CURLUPART_URL,
                relative.as_ptr(),
                CURLU_NON_SUPPORT_SCHEME,
            )
        }
        .with_easy_context("curl_url_set CURLUPART_URL")?;
        Ok(())
    }

    /// Returns the full URL held by the handle.
    pub fn get_url(&self) -> Result<String, CurlCodeContext> {
        self.get(CURLUPART_URL, "curl_url_get CURLUPART_URL")
    }

    fn get(&self, part: c_int, context: &'static str) -> Result<String, CurlCodeContext> {
        self.get_with_flags(part, 0, context)
    }

    fn get_with_flags(
        &self,
        part: c_int,
        flags: u32,
        context: &'static str,
    ) -> Result<String, CurlCodeContext> {
        let mut value = null_mut();
        unsafe { curl_url_get(self.0.as_ptr(), part, &mut value, flags) }
            .with_easy_context(context)?;
        if value.is_null() {
            return Err(curl_sys::CURLE_BAD_FUNCTION_ARGUMENT
                .with_easy_context(context)
                .unwrap_err());
        }
        struct CurlString(*mut c_char);
        impl Drop for CurlString {
            fn drop(&mut self) {
                unsafe { curl_sys::curl_free(self.0 as _) };
            }
        }
        let value = CurlString(value);
        let value = unsafe { CStr::from_ptr(value.0) };
        Ok(value.to_string_lossy().into_owned())
    }

    /// Returns the scheme of the URL, e.g. `https`.
    #[cfg(feature = "cookies")]
    pub fn scheme(&self) -> Result<String, CurlCodeContext> {
        self.get(CURLUPART_SCHEME, "curl_url_get CURLUPART_SCHEME")
    }

    /// Returns the host of the URL, without port.
    ///
    /// The host is normalized to punycode via `CURLU_PUNYCODE` so that IDNA
    /// domains (e.g. `münchen.de` vs `xn--mnchen-3ya.de`) compare consistently
    /// in cookie domain matching. Falls back to the raw host when the libcurl
    /// build lacks IDN support.
    #[cfg(feature = "cookies")]
    pub fn host(&self) -> Result<String, CurlCodeContext> {
        match self.get_with_flags(
            CURLUPART_HOST,
            CURLU_PUNYCODE,
            "curl_url_get CURLUPART_HOST",
        ) {
            Ok(host) => Ok(host),
            Err(_) => self.get(CURLUPART_HOST, "curl_url_get CURLUPART_HOST"),
        }
    }

    /// Returns the path of the URL.
    #[cfg(feature = "cookies")]
    pub fn path(&self) -> Result<String, CurlCodeContext> {
        self.get(CURLUPART_PATH, "curl_url_get CURLUPART_PATH")
    }
}
