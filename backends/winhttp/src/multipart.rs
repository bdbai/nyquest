//! Multipart form body generation.
//!
//! This implementation follows the pattern from nsurlsession backend,
//! supporting both bytes and stream parts.

use std::borrow::Cow;

use nyquest_interface::{Part, PartBody};

#[cfg(any(feature = "blocking-stream", feature = "async-stream"))]
use crate::stream::DataOrStream;

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

// Header only, no boundary (and surrounding CRLF) or content
#[cfg(any(feature = "blocking-stream", feature = "async-stream"))]
fn estimate_part_header_size<S>(part: &Part<S>) -> usize {
    let mut size = 72
        + part.name.len()
        + part.filename.as_ref().map(|s| s.len()).unwrap_or_default()
        + part.content_type.len();
    size += part
        .headers
        .iter()
        .map(|(k, v)| k.len() + v.len() + 4)
        .sum::<usize>();
    size
}

// boundary + headers + content + CRLF + boundary + headers + ... + headers + stream(excluded) + CRLF
// + --boundary--CRLF if we reach the end
#[cfg(any(feature = "blocking-stream", feature = "async-stream"))]
fn estimate_next_data_chunk_size_until_stream<S>(boundary: &str, parts: &[Part<S>]) -> usize {
    let mut contents_and_one_stream_groups =
        parts.split_inclusive(|p| matches!(p.body, PartBody::Stream { .. }));
    let contents_and_one_stream = contents_and_one_stream_groups.next().unwrap_or_default();
    let trailing = if contents_and_one_stream_groups.next().is_none() {
        boundary.len() + 8
    } else {
        Default::default()
    };
    trailing
        + contents_and_one_stream
            .iter()
            .map(|part| {
                let common_size = estimate_part_header_size(part) + boundary.len() + 6;
                if let PartBody::Bytes { content } = &part.body {
                    content.len() + common_size
                } else {
                    common_size
                }
            })
            .sum::<usize>()
}

/// Generates a multipart body from parts, supporting both bytes and stream parts.
///
/// Returns a vector of `DataOrStream` items that should be written in order.
/// For parts with stream bodies, the stream is included in the output.
#[cfg(any(feature = "blocking-stream", feature = "async-stream"))]
pub(crate) fn generate_multipart_body<S>(
    boundary: &str,
    mut parts: Vec<Part<S>>,
) -> Vec<DataOrStream<S>> {
    let stream_count = parts
        .iter()
        .filter(|part| matches!(part.body, PartBody::Stream { .. }))
        .count();
    let mut ret = Vec::with_capacity(2 * stream_count + 1);
    let mut contents_and_one_stream_groups =
        parts.split_inclusive_mut(|p| matches!(p.body, PartBody::Stream { .. }));
    let mut contents_and_one_stream = contents_and_one_stream_groups.next().unwrap_or_default();
    let mut body = Vec::with_capacity(estimate_next_data_chunk_size_until_stream(
        boundary,
        contents_and_one_stream_groups.next().unwrap_or_default(),
    ));
    'group: loop {
        for part in contents_and_one_stream {
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

            // Content - replace with dummy to take ownership of stream
            match std::mem::replace(
                &mut part.body,
                PartBody::Bytes {
                    content: b"".into(),
                },
            ) {
                PartBody::Bytes { content } => {
                    body.extend_from_slice(&content);
                    body.extend_from_slice(b"\r\n");
                }
                PartBody::Stream(content) => {
                    ret.push(DataOrStream::Data(body));
                    ret.push(DataOrStream::Stream(content));
                    contents_and_one_stream =
                        contents_and_one_stream_groups.next().unwrap_or_default();
                    body = Vec::with_capacity(
                        estimate_next_data_chunk_size_until_stream(
                            boundary,
                            contents_and_one_stream,
                        ) + boundary.len(),
                    );
                    body.extend_from_slice(b"\r\n");
                    continue 'group;
                }
            }
        }
        break;
    }

    // Final boundary
    body.extend_from_slice(b"--");
    body.extend_from_slice(boundary.as_bytes());
    body.extend_from_slice(b"--\r\n");
    ret.push(DataOrStream::Data(body));

    ret
}

/// Generates a multipart body from parts as bytes only (no streaming support).
///
/// Used when stream features are not enabled. Ignores stream parts.
#[cfg(not(any(feature = "blocking-stream", feature = "async-stream")))]
pub(crate) fn generate_multipart_body_bytes<S>(boundary: &str, mut parts: Vec<Part<S>>) -> Vec<u8> {
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

        // Content - only handle bytes, skip streams
        match &part.body {
            PartBody::Bytes { content } => {
                body.extend_from_slice(content);
                body.extend_from_slice(b"\r\n");
            }
            PartBody::Stream(_) => {
                // Skip stream parts in non-streaming mode
                body.extend_from_slice(b"\r\n");
            }
        }
    }

    // Final boundary
    body.extend_from_slice(b"--");
    body.extend_from_slice(boundary.as_bytes());
    body.extend_from_slice(b"--\r\n");

    body
}
