use std::borrow::Cow;

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
