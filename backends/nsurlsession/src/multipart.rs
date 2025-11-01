use std::borrow::Cow;

use nyquest_interface::{Part, PartBody};

use crate::stream::DataOrStream;

unsafe extern "C" {
    fn arc4random() -> u32;
}

pub fn generate_multipart_boundary() -> String {
    let [rnd1, rnd2] = unsafe { [arc4random(), arc4random()] };
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
fn estimate_next_data_chunk_size_until_stream<S>(boundary: &str, parts: &[Part<S>]) -> usize {
    let mut contents_and_one_stream_groups =
        parts.split_inclusive(|p| matches!(p.body, PartBody::Stream { .. }));
    let contents_and_one_stream = contents_and_one_stream_groups.next().unwrap_or_default();
    let trailing = contents_and_one_stream_groups
        .next()
        .is_none()
        .then_some(boundary.len() + 8)
        .unwrap_or_default();
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

pub fn generate_multipart_body<S>(boundary: &str, mut parts: Vec<Part<S>>) -> Vec<DataOrStream<S>> {
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
            body.extend_from_slice(b"--");
            body.extend_from_slice(boundary.as_bytes());
            body.extend_from_slice(b"\r\nContent-Disposition: form-data; name=\"");
            body.extend_from_slice(part.name.as_bytes());
            body.extend_from_slice(b"\"");
            if let Some(mut filename) = part.filename.clone() {
                body.extend_from_slice(b"; filename=\"");
                const STRIPPED_CHARS: &[char] = &['"', '\\', '/'];
                if filename.contains(STRIPPED_CHARS) {
                    filename = filename.replace(STRIPPED_CHARS, "_").into();
                }
                body.extend_from_slice(filename.as_bytes());
                body.extend_from_slice(b"\"");
            }
            body.extend_from_slice(b"\r\n");
            body.extend_from_slice(b"Content-Type: ");
            body.extend_from_slice(part.content_type.as_bytes());
            body.extend_from_slice(b"\r\n");
            for (mut k, mut v) in part.headers.clone() {
                quick_escape_header(&mut k, &mut v);
                body.extend_from_slice(k.as_bytes());
                body.extend_from_slice(b": ");
                body.extend_from_slice(v.as_bytes());
                body.extend_from_slice(b"\r\n");
            }
            body.extend_from_slice(b"\r\n");
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
                            &contents_and_one_stream,
                        ) + boundary.len(),
                    );
                    body.extend_from_slice(b"\r\n");
                    continue 'group;
                }
            }
        }
        break;
    }
    body.extend_from_slice(b"--");
    body.extend_from_slice(boundary.as_bytes());
    body.extend_from_slice(b"--\r\n");
    ret.push(DataOrStream::Data(body));

    ret
}
