//! Client-specific customization of libcurl easy handles.
//!
//! This module is the public surface of the customization: [`CurlHandle`] is the easy handle a
//! customization configures — handed to it together with the request being made — and
//! [`Customizer`] is what a client is built with. The machinery behind them belongs to the handle
//! chain, and lives in [`crate::curl_ng::easy`].
//!
//! A customization is not stored as one erased value. The two shapes a request can have — a
//! blocking one, and an asynchronous one — are known when a backend is built, so the value a
//! caller hands over is instantiated there once per shape, and the two monomorphic callables are
//! what the backend holds. Nothing is dispatched dynamically on the way to the caller's code, and
//! nothing about the stream type reaches the caller's signature.

use std::any::Any;
use std::borrow::Cow;
use std::ffi::c_long;
use std::pin::Pin;

use nyquest_interface::Request;

use crate::curl_ng::easy::AsRawEasyMut;
use crate::curl_ng::{
    easy::{EasyWithCustomData, RawEasy},
    CurlCodeContext, CurlStringList,
};

/// Configures the easy handle of a request, with the request itself in hand.
///
/// Implement this for a type and pass it to
/// [`CurlBackend::customize`](crate::CurlBackend::customize):
///
/// ```rust
/// use nyquest_backend_curl::{curl_sys, CurlBackend, CurlHandle, HandleCustomizer};
///
/// struct Customizer;
///
/// impl HandleCustomizer for Customizer {
///     fn customize_handle(
///         &self,
///         mut handle: CurlHandle<'_>,
///     ) -> Result<(), nyquest_backend_curl::CurlCodeContext> {
///         // Safety: `CURLOPT_TCP_KEEPALIVE` takes a `long`.
///         unsafe { handle.setopt_long(curl_sys::CURLOPT_TCP_KEEPALIVE, 1)? };
///         Ok(())
///     }
/// }
///
/// let backend = CurlBackend::customize(Customizer);
/// ```
///
/// The same customizer serves both the blocking and the async client when both are built from
/// the same backend.
///
/// The [`CurlHandle`] is the nyquest-managed easy handle of the request, and the request is the one
/// being made. Both are borrowed for the duration of the call: the handle must not be stored or
/// cleaned up, and neither must the request. The request is handed over as nyquest's built-in
/// population left it, with its body already consumed.
pub trait HandleCustomizer {
    /// Applies the customization to every new handle.
    ///
    /// The default implementation is a no-op. When [`Self::customize_handle_with_request`] is overridden, this method is not called.
    fn customize_handle(&self, _handle: CurlHandle<'_>) -> Result<(), CurlCodeContext> {
        Ok(())
    }
    /// Applies the customization to every new handle given a request.
    ///
    /// The default implementation calls [`Self::customize_handle`].
    fn customize_handle_with_request<S>(
        &self,
        handle: CurlHandle<'_>,
        _request: &Request<S>,
    ) -> Result<(), CurlCodeContext> {
        self.customize_handle(handle)
    }
}

#[derive(Clone, Copy, Default, Debug)]
pub(crate) struct NoopHandleCustomizer;

impl HandleCustomizer for NoopHandleCustomizer {
    fn customize_handle(&self, _handle: CurlHandle<'_>) -> Result<(), CurlCodeContext> {
        Ok(())
    }
}

/// The easy handle of a request, for use in a [`HandleCustomizer`] to set
/// custom libcurl options.
pub struct CurlHandle<'a> {
    easy: Pin<&'a mut EasyWithCustomData<RawEasy>>,
}

impl<'a> CurlHandle<'a> {
    pub(crate) fn new(easy: Pin<&'a mut EasyWithCustomData<RawEasy>>) -> Self {
        Self { easy }
    }
}

impl CurlHandle<'_> {
    /// Returns a raw pointer to the underlying `CURL` handle.
    ///
    /// The pointer is valid for the duration of the customizer call. It may be used to call
    /// libcurl functions that are not covered by the typed setters below, such as
    /// `curl_easy_getinfo`.
    #[inline]
    pub fn as_raw(&mut self) -> *mut curl_sys::CURL {
        self.easy.as_mut().as_raw_easy_mut().raw()
    }

    /// Sets a pointer-typed option by moving an owned value into the handle.
    ///
    /// The value is boxed and stored alongside the easy handle, keeping the pointer libcurl holds
    /// valid for the lifetime of the handle. libcurl is given the address of the boxed value, so
    /// the option must be one that libcurl reads as a raw pointer (a `CURLOPTTYPE_OBJECTPOINT`
    /// option).
    ///
    /// # Safety
    ///
    /// `opt` must be an option that libcurl reads as a raw data pointer, and the type `T` must
    /// match what libcurl expects to find at that address for the given option.
    ///
    /// The option must not interfere with the backend's own use of the handle.
    pub unsafe fn setopt_ptr_owned<T: Any + Send + Sync>(
        &mut self,
        opt: curl_sys::CURLoption,
        value: T,
    ) -> Result<(), CurlCodeContext> {
        unsafe { self.easy.as_mut().setopt_any(opt, Box::new(value)) }
    }

    /// Sets a linked-list option by moving an owned [`CurlStringList`] into the handle.
    ///
    /// The list is stored alongside the easy handle and given to libcurl as a `struct curl_slist *`
    /// pointer. The option must be a `CURLOPTTYPE_SLISTPOINT` option (such as
    /// `CURLOPT_HTTPHEADER`).
    ///
    /// # Safety
    ///
    /// `opt` must be an option that libcurl reads as a `struct curl_slist *`.
    ///
    /// The option must not interfere with the backend's own use of the handle.
    #[inline]
    pub unsafe fn setopt_slist(
        &mut self,
        opt: curl_sys::CURLoption,
        list: CurlStringList,
    ) -> Result<(), CurlCodeContext> {
        unsafe { self.easy.as_mut().setopt_slist(opt, list) }
    }

    /// Sets a blob option by moving an owned or borrowed buffer into the handle.
    ///
    /// The buffer (a `&'static [u8]` or a `Vec<u8>`) is wrapped in a `curl_blob` and stored
    /// alongside the handle. libcurl is given `CURL_BLOB_NOCOPY`, meaning it reads the address the
    /// handle keeps valid; the buffer is not copied by libcurl. The option must be a
    /// `CURLOPTTYPE_BLOBPOINT` option (such as `CURLOPT_CAINFO_BLOB`).
    ///
    /// # Safety
    ///
    /// `opt` must be an option that libcurl reads as a `struct curl_blob *`. The data must be
    /// valid for the purpose of the option (e.g. a DER- or PEM-encoded certificate).
    ///
    /// The option must not interfere with the backend's own use of the handle.
    pub unsafe fn setopt_blob(
        &mut self,
        opt: curl_sys::CURLoption,
        data: impl Into<Cow<'static, [u8]>>,
    ) -> Result<(), CurlCodeContext> {
        unsafe { self.easy.as_mut().setopt_blob(opt, data.into()) }
    }

    /// Sets a long (integer) option.
    ///
    /// Most boolean and enumerated libcurl options are `CURLOPTTYPE_LONG` options, as are options
    /// such as `CURLOPT_TIMEOUT_MS` and `CURLOPT_MAXREDIRS`.
    ///
    /// # Safety
    ///
    /// `opt` must be a `CURLOPTTYPE_LONG` option.
    ///
    /// The option must not interfere with the backend's own use of the handle.
    #[inline]
    pub unsafe fn setopt_long(
        &mut self,
        opt: curl_sys::CURLoption,
        val: c_long,
    ) -> Result<(), CurlCodeContext> {
        unsafe { self.easy.as_mut().setopt_long_raw(opt, val) }
    }

    /// Sets a `curl_off_t` (64-bit integer) option.
    ///
    /// Options such as `CURLOPT_INFILESIZE_LARGE` and `CURLOPT_POSTFIELDSIZE_LARGE` are
    /// `CURLOPTTYPE_OFF_T` options.
    ///
    /// # Safety
    ///
    /// `opt` must be a `CURLOPTTYPE_OFF_T` option.
    ///
    /// The option must not interfere with the backend's own use of the handle.
    #[inline]
    pub unsafe fn setopt_off_t(
        &mut self,
        opt: curl_sys::CURLoption,
        val: curl_sys::curl_off_t,
    ) -> Result<(), CurlCodeContext> {
        unsafe { self.easy.as_mut().setopt_off_t_raw(opt, val) }
    }

    /// Sets a pointer option with a raw pointer.
    ///
    /// The pointer is given directly to `curl_easy_setopt` without being stored by the handle.
    /// The caller is responsible for keeping the pointed-to data alive for as long as libcurl may
    /// read it (typically until the request completes).
    ///
    /// # Safety
    ///
    /// `opt` must be an option that libcurl reads as a raw pointer, and `val` must remain valid
    /// and accessible as long as libcurl may read it.
    ///
    /// For string options that are copied by libcurl, `val` must point to a valid NUL-terminated
    /// C string.
    #[inline]
    pub unsafe fn setopt_ptr_raw<T: Sized>(
        &mut self,
        opt: curl_sys::CURLoption,
        val: *const T,
    ) -> Result<(), CurlCodeContext> {
        unsafe { self.easy.as_mut().setopt_ptr_raw(opt, val.cast()) }
    }
}
