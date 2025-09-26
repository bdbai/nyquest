use std::{borrow::Cow, mem::transmute_copy};

use curl::easy::List;
use libc::c_char;

pub(super) fn transform_cow_str_to_c_str(val: &mut Cow<'_, str>) -> *const c_char {
    if val.ends_with('\0') {
        // Quick path: if the string ends with a null byte, we can just use
        // the pointer directly.
    } else {
        let mut s = std::mem::take(val).into_owned();
        s.push('\0');
        *val = Cow::Owned(s);
    };
    val.as_ptr() as *const c_char
}

pub(super) fn list_to_raw(list: Option<&List>) -> *mut curl_sys::curl_slist {
    list.as_ref().map_or(std::ptr::null_mut(), |l| {
        // TODO: rewrite our own List wrapper
        assert_eq!(
            size_of::<List>(),
            size_of::<*mut curl_sys::curl_slist>(),
            "List size is not equal to curl_slist pointer size"
        );
        unsafe { transmute_copy::<List, *mut curl_sys::curl_slist>(l) }
    })
}
