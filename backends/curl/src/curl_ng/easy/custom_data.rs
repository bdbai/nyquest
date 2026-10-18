//! The layer of the handle chain that owns the data of the options set on a handle.
//!
//! The customization hook of a client is one value shared by every handle of that client, so it
//! cannot own the data that an option has to keep alive: a `struct curl_slist`, a
//! `struct curl_blob` or an application data pointer is read by libcurl again during the transfer,
//! long after the hook has returned.
//!
//! [`EasyWithCustomData`] solves that by being both the owner of that data and the layer the hook
//! configures. Every setter goes through it, so the two halves that have to stay in step — the
//! pointer libcurl is given for an option, and the box that owns what it points at — are written in
//! one step, together. That one path is the only way the storage is written, which is what keeps a
//! value from being separated from the option that names it.
//!
//! This module is crate-internal in practice: the handle chain of `curl_ng` is not reachable from
//! outside the crate, and neither this layer nor [`CustomizeEasy`] is re-exported. The parts of the
//! customization that *are* part of the public API — [`CurlHandle`](crate::CurlHandle) and
//! [`Customizer`](crate::Customizer), which is what a caller actually configures — live in
//! [`crate::custom`].

use std::any::Any;
use std::borrow::Cow;
use std::ffi::{c_char, c_long, c_void};
use std::fmt;
use std::pin::Pin;

use pin_project_lite::pin_project;

use crate::curl_ng::easy::{AsRawEasyMut, RawEasy};
use crate::curl_ng::{CurlCodeContext, CurlStringList, WithCurlCodeContext as _};

pin_project! {
    /// An easy handle together with the data it owns on behalf of the options set on it.
    ///
    /// This is the innermost wrapper of the handle chain, so that the option that names a value and
    /// the value itself belong to the same layer. It is also the only layer that knows how to set
    /// an option, which is what lets every setter register a value and give libcurl the pointer to
    /// it in one step.
    ///
    /// Like every other layer of the chain, this one is `Unpin` exactly when the handle inside it
    /// is, which is what `#[pin]` makes the projection derive. That conditionality is the property
    /// that matters: `E` reaches [`OwnedEasyWithErrorBuf`], whose error buffer libcurl writes into
    /// at an address that must not change, so a wrapper that declared itself `Unpin` regardless of
    /// `E` would let [`Pin::into_inner`] move such a handle out from under that address.
    pub struct EasyWithCustomData<E> {
        // The field order is load-bearing: fields are dropped in declaration order, so `easy` —
        // and with it `curl_easy_cleanup`, which reads the option strings of the handle — is torn
        // down before `custom_data` is freed.
        #[pin]
        easy: E,
        slots: CustomDataVec,
    }
}

/// Stores the data that an easy handle owns on behalf of the options set on it.
///
/// The type is crate-private, but that is a matter of API surface rather than of safety: it is only
/// ever written through the layer that wraps the handle,
/// [`EasyWithCustomData`](crate::curl_ng::easy::EasyWithCustomData), which sets the option of the
/// handle and registers the value in the same step, so the two cannot drift apart.
#[derive(Debug, Default)]
pub(crate) struct CustomDataVec {
    slots: Vec<SlotItem>,
}

/// An entry of [`CustomData`].
#[derive(Debug)]
struct SlotItem {
    option: curl_sys::CURLoption,
    value: SlotItemValue,
}

/// The value is boxed, so its address does not change when the storage grows or when the
/// storage itself moves. That address is the one libcurl is given for [`Slot::option`].
#[derive(Debug)]
pub(crate) enum SlotItemValue {
    Slist(CurlStringList),
    Blob(OwnedBlob),
    Any(Box<dyn Any + Send>),
}
impl<E> EasyWithCustomData<E> {
    pub fn new(easy: E) -> Self {
        Self {
            easy,
            slots: CustomDataVec::default(),
        }
    }
}

impl<E: AsRawEasyMut> AsRawEasyMut for EasyWithCustomData<E> {
    fn init(self: Pin<&mut Self>) -> Result<(), CurlCodeContext> {
        self.project().easy.init()
    }

    fn as_raw_easy_mut(self: Pin<&mut Self>) -> Pin<&mut RawEasy> {
        self.project().easy.as_raw_easy_mut()
    }

    fn reset(self: Pin<&mut Self>) -> Result<(), CurlCodeContext> {
        let this = self.project();
        this.easy.reset()?;
        this.slots.clear();
        Ok(())
    }
}

impl<E: AsRawEasyMut> EasyWithCustomData<E> {
    /// Moves `item` into the storage of this handle as the item of `opt`, and gives libcurl the
    /// pointer `ptr` derives from it.
    ///
    /// This is the pairing the module exists for: the option and the data behind it are written in
    /// one step, and the value is dropped again if the handle refuses the option.
    ///
    /// # Safety
    ///
    /// `opt` must read a pointer of the same kind as `item`.
    unsafe fn setopt_owned(
        self: Pin<&mut Self>,
        opt: curl_sys::CURLoption,
        item: SlotItemValue,
        context: &'static str,
    ) -> Result<(), CurlCodeContext> {
        let this = self.project();

        let mut easy = this.easy.as_raw_easy_mut();
        let ptr = item.as_ptr();
        // FIXME: the pointer to the OwnedBlob points to a temporary variable that is only valid
        // before being moved into the storage. Find a way to avoid dangling the pointer.
        unsafe { RawEasy::setopt_ptr(easy.as_mut(), opt, ptr) }.with_easy_context(context)?;

        struct Guard<'a> {
            easy: Pin<&'a mut RawEasy>,
            opt: curl_sys::CURLoption,
        }

        impl Drop for Guard<'_> {
            fn drop(&mut self) {
                // The handle did not take the option, so the storage should not keep the value
                // either.
                let res = unsafe { self.easy.as_mut().setopt_ptr(self.opt, std::ptr::null()) };
                assert_eq!(
                    res,
                    curl_sys::CURLE_OK,
                    "reverting a easy option to null due to an option object fails to be inserted"
                );
            }
        }
        let easy_guard = Guard { easy, opt };
        let old_item = this.slots.put(opt, item);
        std::mem::forget(easy_guard);
        drop(old_item);
        Ok(())
    }

    pub(crate) unsafe fn setopt_any(
        self: Pin<&mut Self>,
        opt: curl_sys::CURLoption,
        value: Box<dyn Any + Send>,
    ) -> Result<(), CurlCodeContext> {
        let item = SlotItemValue::Any(value);
        unsafe { self.setopt_owned(opt, item, "setopt (custom, ptr)") }
    }

    pub(crate) unsafe fn setopt_slist(
        self: Pin<&mut Self>,
        opt: curl_sys::CURLoption,
        list: CurlStringList,
    ) -> Result<(), CurlCodeContext> {
        let item = SlotItemValue::Slist(list);
        unsafe { self.setopt_owned(opt, item, "setopt (custom, slist)") }
    }

    pub(crate) unsafe fn setopt_blob(
        self: Pin<&mut Self>,
        opt: curl_sys::CURLoption,
        data: Cow<'static, [u8]>,
    ) -> Result<(), CurlCodeContext> {
        let item = SlotItemValue::Blob(OwnedBlob::new(data));
        unsafe { self.setopt_owned(opt, item, "setopt (custom, blob)") }
    }

    pub(crate) unsafe fn setopt_ptr_raw(
        self: Pin<&mut Self>,
        opt: curl_sys::CURLoption,
        value: *const c_void,
    ) -> Result<(), CurlCodeContext> {
        let easy = self.as_raw_easy_mut();
        unsafe { RawEasy::setopt_ptr(easy, opt, value.cast()) }
            .with_easy_context("setopt (custom, pointer)")
    }

    pub(crate) unsafe fn setopt_long_raw(
        self: Pin<&mut Self>,
        opt: curl_sys::CURLoption,
        value: c_long,
    ) -> Result<(), CurlCodeContext> {
        let easy = self.as_raw_easy_mut();
        unsafe { RawEasy::setopt_long(easy, opt, value) }.with_easy_context("setopt (custom, long)")
    }

    pub(crate) unsafe fn setopt_off_t_raw(
        self: Pin<&mut Self>,
        opt: curl_sys::CURLoption,
        value: curl_sys::curl_off_t,
    ) -> Result<(), CurlCodeContext> {
        let easy = self.as_raw_easy_mut();
        unsafe { RawEasy::setopt_off_t(easy, opt, value) }
            .with_easy_context("setopt (custom, off_t)")
    }
}

impl SlotItemValue {
    pub(crate) fn as_ptr(&self) -> *const c_char {
        match self {
            SlotItemValue::Slist(list) => list.raw() as *const _ as *const _,
            SlotItemValue::Blob(blob) => &blob.raw as *const curl_sys::curl_blob as *const _,
            SlotItemValue::Any(any) => any.as_ref() as *const (dyn Any + Send) as *const _,
        }
    }
}

impl CustomDataVec {
    /// Moves `data` into the storage as the value of `option`, replacing and returning whatever
    /// was there.
    ///
    /// This does not talk to an easy handle, so it is not how an option is set: the wrapper that
    /// owns this storage is what pairs the two, and only inside it does a pointer registered here
    /// belong to the option it names.
    pub(crate) fn put(
        &mut self,
        option: curl_sys::CURLoption,
        item: SlotItemValue,
    ) -> Option<SlotItemValue> {
        let item = SlotItem {
            option,
            value: item,
        };
        if let Some(slot) = self.slots.iter_mut().find(|it| it.option == option) {
            Some(std::mem::replace(slot, item).value)
        } else {
            self.slots.push(item);
            None
        }
    }

    /// Frees every value owned by the handle.
    ///
    /// The caller is responsible for making sure that libcurl no longer reads them, which is the
    /// case after [`curl_easy_reset`](curl_sys::curl_easy_reset) or before
    /// [`curl_easy_cleanup`](curl_sys::curl_easy_cleanup).
    pub(crate) fn clear(&mut self) {
        self.slots.clear();
    }

    /// Returns how many values the handle owns.
    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.slots.len()
    }
}

/// A `struct curl_blob` together with the buffer it points to.
pub(crate) struct OwnedBlob {
    raw: curl_sys::curl_blob,
    /// Borrowed data has a fixed address for the `'static` lifetime it promises, and owned data
    /// lives in a `Vec` that nothing mutates while the handle holds it, so the address in
    /// [`OwnedBlob::raw`] stays valid either way.
    data: Cow<'static, [u8]>,
}

impl OwnedBlob {
    fn new(data: Cow<'static, [u8]>) -> Self {
        // Take the address before the buffer moves into the struct. Borrowed data keeps its
        // address, and the `Vec` of owned data keeps the address of its allocation when the `Cow`
        // itself moves, so the address is the buffer's for as long as the handle holds it.
        let (ptr, len) = { (data.as_ptr(), data.len()) };
        Self {
            raw: curl_sys::curl_blob {
                data: ptr.cast_mut().cast(),
                len,
                // The buffer is owned by the handle, so libcurl must not try to own or copy it.
                flags: curl_sys::CURL_BLOB_NOCOPY,
            },
            data,
        }
    }
}

// Safety: the pointer in `raw` points into `_data`, which is owned by the same value.
unsafe impl Send for OwnedBlob {}
unsafe impl Sync for OwnedBlob {}

impl fmt::Debug for OwnedBlob {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("OwnedBlob")
            .field("raw.data", &self.raw.data)
            .field("raw.len", &self.raw.len)
            .field("raw.flags", &self.raw.flags)
            .field("_data", &self.data)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Setting an option and owning what it reads are one step, so the storage of a handle can
    /// never hold a value that the handle was not told about.
    #[test]
    fn setting_an_option_registers_the_value_it_reads() {
        let mut easy = Box::pin(EasyWithCustomData::new(RawEasy::new()));

        // Safety: `CURLOPT_RESOLVE` takes a `struct curl_slist`.
        unsafe {
            easy.as_mut()
                .setopt_slist(curl_sys::CURLOPT_RESOLVE, CurlStringList::default())
        }
        .unwrap();

        // The handle owns the list the option was given, and the pointer libcurl holds is the one
        // the handle registered for it.
        assert_eq!(easy.slots.len(), 1);
        assert!(easy
            .slots
            .slots
            .iter()
            .filter(|item| item.option == curl_sys::CURLOPT_RESOLVE)
            .find(|item| matches!(&item.value, SlotItemValue::Slist(_)))
            .is_some());
    }

    /// An option the handle refuses leaves nothing behind, so the storage cannot hold a value that
    /// no option refers to.
    #[test]
    fn a_rejected_option_leaves_nothing_registered() {
        let bogus = 999_999 as curl_sys::CURLoption;
        let mut easy = Box::pin(EasyWithCustomData::new(RawEasy::new()));

        // Safety: the option number is bogus on purpose; libcurl rejects it before reading the
        // value it was given.
        let result = unsafe { easy.as_mut().setopt_slist(bogus, CurlStringList::default()) };

        assert!(result.is_err());
        assert_eq!(easy.slots.len(), 0);
    }
}
