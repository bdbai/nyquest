use std::borrow::Cow;
use std::ffi::CStr;
use std::marker::PhantomData;
use std::{fmt, ptr};

use crate::curl_ng::ffi::transform_cow_str_to_c_str;

/// Represents a `curl_slist` in libcurl, which is a linked list of C-style strings.
pub struct CurlStringList {
    raw: *mut curl_sys::curl_slist,
}

unsafe impl Send for CurlStringList {}
unsafe impl Sync for CurlStringList {}

impl Default for CurlStringList {
    fn default() -> Self {
        Self {
            raw: ptr::null_mut(),
        }
    }
}

impl CurlStringList {
    /// Appends a new string into this list.
    pub fn append<'s>(&mut self, data: impl Into<Cow<'s, str>>) {
        let mut data = data.into();
        let raw_str = transform_cow_str_to_c_str(&mut data);
        unsafe {
            let raw = curl_sys::curl_slist_append(self.raw, raw_str);
            assert!(!raw.is_null());
            self.raw = raw;
        }
    }

    /// Returns a raw pointer to the underlying `curl_slist`.
    pub fn raw(&self) -> *mut curl_sys::curl_slist {
        self.raw
    }
}

impl Clone for CurlStringList {
    fn clone(&self) -> Self {
        let mut copy = Self::default();
        let mut from_slist = self.raw;
        let to_slist = &mut copy.raw;

        while !from_slist.is_null() {
            unsafe {
                let new_slist = curl_sys::curl_slist_append(*to_slist, (*from_slist).data);
                assert!(!new_slist.is_null());
                *to_slist = new_slist;
                from_slist = (*from_slist).next;
            }
        }
        copy
    }
}

impl Drop for CurlStringList {
    fn drop(&mut self) {
        unsafe { curl_sys::curl_slist_free_all(self.raw) }
    }
}

#[derive(Clone)]
pub struct Iter<'a> {
    cur: *mut curl_sys::curl_slist,
    _me: PhantomData<&'a CurlStringList>,
}

impl fmt::Debug for CurlStringList {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_list()
            .entries(self.into_iter().map(String::from_utf8_lossy))
            .finish()
    }
}

impl<'a> IntoIterator for &'a CurlStringList {
    type IntoIter = Iter<'a>;
    type Item = &'a [u8];

    fn into_iter(self) -> Iter<'a> {
        Iter {
            cur: self.raw,
            _me: PhantomData,
        }
    }
}

impl<'a> Iterator for Iter<'a> {
    type Item = &'a [u8];

    fn next(&mut self) -> Option<&'a [u8]> {
        if self.cur.is_null() {
            return None;
        }

        unsafe {
            let ret = Some(CStr::from_ptr((*self.cur).data).to_bytes());
            self.cur = (*self.cur).next;
            ret
        }
    }
}

impl<'a> fmt::Debug for Iter<'a> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_list()
            .entries(self.clone().map(String::from_utf8_lossy))
            .finish()
    }
}
