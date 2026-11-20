//! Cookie value encoding helpers shared by the native and WASM implementations.

use percent_encoding::{percent_decode_str, utf8_percent_encode, AsciiSet, CONTROLS};

/// Characters in a cookie value that must be percent-encoded.
///
/// This keeps the encoding behavior aligned with the previous hand-rolled implementation:
/// ASCII alphanumerics and symbols other than `%`, `;`, `"`, `,`, `\` and whitespace pass
/// through, while control characters, whitespace, the listed delimiters and all non-ASCII
/// bytes (handled unconditionally by `utf8_percent_encode`) are encoded.
const COOKIE_VALUE_ENCODE_SET: &AsciiSet = &CONTROLS
    .add(b'%')
    .add(b';')
    .add(b'"')
    .add(b',')
    .add(b'\\')
    .add(b' ');

pub(crate) fn encode_cookie_value(value: &str) -> String {
    utf8_percent_encode(value, COOKIE_VALUE_ENCODE_SET).to_string()
}

pub(crate) fn decode_cookie_value(value: &str) -> String {
    percent_decode_str(value).decode_utf8_lossy().into_owned()
}
