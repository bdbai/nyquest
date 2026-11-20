use std::{borrow::Cow, ffi::CString};

use nyquest_interface::Error as NyquestError;

use crate::curl_ng::url::CurlUrl;

pub(crate) fn parse_base_url(base: &str) -> Result<CurlUrl, NyquestError> {
    let base_cstr = CString::new(base).map_err(|_| NyquestError::InvalidUrl)?;
    CurlUrl::parse_base(&base_cstr).map_err(|_| NyquestError::InvalidUrl)
}

pub(crate) fn join_url(base: Option<&CurlUrl>, relative: &str) -> Result<String, NyquestError> {
    let relative_cstr = CString::new(relative).map_err(|_| NyquestError::InvalidUrl)?;
    CurlUrl::join(base, &relative_cstr).map_err(|_| NyquestError::InvalidUrl)
}

pub(crate) fn form_url_encode<'s>(
    easy: *mut curl_sys::CURL,
    input: impl Into<Cow<'s, str>>,
    output: &mut Vec<u8>,
) {
    use std::ffi::{c_char, CStr};

    let input = input.into();
    if input.is_empty() {
        return;
    }

    struct CurlString(*mut c_char);
    impl Drop for CurlString {
        fn drop(&mut self) {
            unsafe {
                curl_sys::curl_free(self.0 as _);
            }
        }
    }

    unsafe {
        let encoded = curl_sys::curl_easy_escape(easy, input.as_ptr() as _, input.len() as _);
        assert!(!encoded.is_null());
        let encoded = CurlString(encoded);
        let mut encoded = CStr::from_ptr(encoded.0).to_bytes();
        while let Some(idx) = memchr::memmem::find(encoded, b"%20") {
            output.extend_from_slice(&encoded[..idx]);
            output.push(b'+');
            encoded = &encoded[idx + 3..];
        }
        output.extend_from_slice(encoded);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn join(base: Option<&str>, relative: &str) -> Result<String, NyquestError> {
        let base = base.map(parse_base_url).transpose()?;
        join_url(base.as_ref(), relative)
    }

    #[test]
    fn test_join_url() {
        crate::init();
        let testcases = [
            (None, "http://example.com", "http://example.com/"),
            (
                None,
                "https://example.com/path?q=1",
                "https://example.com/path?q=1",
            ),
            (Some("http://a.com"), "https://b.com", "https://b.com/"),
            (Some("http://a.com"), "//b.com", "http://b.com/"),
            (Some("https://a.com"), "//b.com", "https://b.com/"),
            (Some("http://a.com"), "/b", "http://a.com/b"),
            (Some("http://a.com/a/b"), "/c", "http://a.com/c"),
            (Some("http://a.com?q=1"), "/c", "http://a.com/c"),
            // RFC 3986 resolution: relative paths merge against the base path,
            // `..`/`.` segments are normalized.
            (Some("http://a.com/a/b"), "c", "http://a.com/a/c"),
            (Some("http://a.com/a/b/"), "c", "http://a.com/a/b/c"),
            (Some("http://a.com/a/b"), "../c", "http://a.com/c"),
            (Some("http://a.com/a/b"), "./c", "http://a.com/a/c"),
            (Some("http://a.com/a/b?q=1"), "c", "http://a.com/a/c"),
            (Some("http://a.com?q=1"), "c", "http://a.com/c"),
            (Some("http://a.com/a/b"), "c?q=1", "http://a.com/a/c?q=1"),
            (Some("ws://a.com"), "/ws", "ws://a.com/ws"),
            (Some("http://a.com"), "ws://b.com/ws", "ws://b.com/ws"),
        ];
        for (base, relative, expected) in testcases {
            assert_eq!(
                join(base, relative).unwrap(),
                expected,
                "{}, {relative}, {expected}",
                base.unwrap_or("None"),
            );
        }
    }

    #[test]
    fn test_join_url_invalid() {
        crate::init();
        // Schemeless base fails fast at parse time.
        assert!(parse_base_url("a.com").is_err());
        assert!(parse_base_url("http://a.com\0").is_err());
        // Relative with NUL is rejected.
        let base = parse_base_url("http://a.com").unwrap();
        assert!(join_url(Some(&base), "/b\0").is_err());
        // Relative path without base has no host to resolve against.
        assert!(join(None, "/only-path").is_err());
        assert!(join(None, "not a url ^").is_err());
    }
}
