//! Multipart form body generation.
//!
//! This implementation follows the pattern from nsurlsession backend.

use std::borrow::Cow;

use nyquest_interface::{Part, PartBody};

/// Generates a random multipart boundary string.
pub(crate) fn generate_multipart_boundary() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};

    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);

    // Use timestamp and a simple counter for uniqueness
    let rnd1 = (timestamp & 0xFFFFFFFF) as u32;
    let rnd2 = ((timestamp >> 32) & 0xFFFFFFFF) as u32;

    format!("----nyquest.boundary.{rnd1:08x}{rnd2:08x}")
}

fn quick_escape_header(key: &mut Cow<'static, str>, value: &mut Cow<'static, str>) {
    if key.contains(':') {
        *key = key.replace(':', "%3A").into();
    }
    static NEW_LINE: &[char] = &['\r', '\n'];
    for s in [key, value] {
        if s.contains(NEW_LINE) {
            *s = s.replace(NEW_LINE, "\\n").into();
        }
    }
}

/// Generates a complete multipart body from parts.
///
/// Note: This implementation does not support streaming parts for simplicity.
/// All parts are collected into a single Vec<u8>.
pub(crate) fn generate_multipart_body<S>(boundary: &str, mut parts: Vec<Part<S>>) -> Vec<u8> {
    let mut body = Vec::new();

    for part in &mut parts {
        // Boundary
        body.extend_from_slice(b"--");
        body.extend_from_slice(boundary.as_bytes());
        body.extend_from_slice(b"\r\n");

        // Content-Disposition header
        body.extend_from_slice(b"Content-Disposition: form-data; name=\"");
        body.extend_from_slice(part.name.as_bytes());
        body.push(b'"');

        if let Some(ref mut filename) = part.filename {
            body.extend_from_slice(b"; filename=\"");
            const STRIPPED_CHARS: &[char] = &['"', '\\', '/'];
            if filename.contains(STRIPPED_CHARS) {
                *filename = filename.replace(STRIPPED_CHARS, "_").into();
            }
            body.extend_from_slice(filename.as_bytes());
            body.push(b'"');
        }
        body.extend_from_slice(b"\r\n");

        // Content-Type header
        body.extend_from_slice(b"Content-Type: ");
        body.extend_from_slice(part.content_type.as_bytes());
        body.extend_from_slice(b"\r\n");

        // Additional headers
        for (mut k, mut v) in part.headers.clone() {
            quick_escape_header(&mut k, &mut v);
            body.extend_from_slice(k.as_bytes());
            body.extend_from_slice(b": ");
            body.extend_from_slice(v.as_bytes());
            body.extend_from_slice(b"\r\n");
        }

        // Empty line before content
        body.extend_from_slice(b"\r\n");

        // Content
        match &part.body {
            PartBody::Bytes { content } => {
                body.extend_from_slice(content);
            }
            PartBody::Stream(_) => {
                // For now, streaming multipart parts are not supported
                // This would require significant complexity with WinHTTP's
                // chunked transfer encoding
            }
        }

        body.extend_from_slice(b"\r\n");
    }

    // Final boundary
    body.extend_from_slice(b"--");
    body.extend_from_slice(boundary.as_bytes());
    body.extend_from_slice(b"--\r\n");

    body
}
