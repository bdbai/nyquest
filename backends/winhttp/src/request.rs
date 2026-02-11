//! Request building utilities for WinHTTP backend.

use std::borrow::Cow;

use nyquest_interface::{Body, Method};
use widestring::u16cstr;

use crate::error::Result;
use crate::handle::{ConnectionHandle, RequestHandle};
use crate::session::WinHttpSession;
use crate::url::ParsedUrl;

/// Prepared request body data (with streaming support).
#[cfg(any(feature = "blocking-stream", feature = "async-stream"))]
pub(crate) enum PreparedBody<S> {
    /// No body
    None,
    /// Complete body data
    Complete(Vec<u8>),
    /// Streaming body with content type and optional content length
    Stream {
        content_type: String,
        stream_parts: Vec<crate::stream::DataOrStream<S>>,
    },
}

/// Prepared request body data (without streaming support).
#[cfg(not(any(feature = "blocking-stream", feature = "async-stream")))]
pub(crate) enum PreparedBody {
    /// No body
    None,
    /// Complete body data
    Complete(Vec<u8>),
}

/// Prepares headers string from request additional_headers only.
#[cfg(any(feature = "blocking-stream", feature = "async-stream"))]
pub(crate) fn prepare_additional_headers<S>(
    additional_headers: &[(Cow<'static, str>, Cow<'static, str>)],
    options: &nyquest_interface::client::ClientOptions,
    body: &PreparedBody<S>,
) -> String {
    let mut headers = String::new();

    // Add default headers
    for (name, value) in &options.default_headers {
        // Skip if overridden in additional_headers
        if !additional_headers
            .iter()
            .any(|(n, _)| n.eq_ignore_ascii_case(name))
        {
            headers.push_str(name);
            headers.push_str(": ");
            headers.push_str(value);
            headers.push_str("\r\n");
        }
    }

    // Add request-specific headers
    for (name, value) in additional_headers {
        headers.push_str(name);
        headers.push_str(": ");
        headers.push_str(value);
        headers.push_str("\r\n");
    }

    // Add Content-Type if needed
    match body {
        PreparedBody::Complete(_) => {
            // Content-Type is handled when preparing body
        }
        PreparedBody::Stream { content_type, .. } => {
            if !additional_headers
                .iter()
                .any(|(n, _)| n.eq_ignore_ascii_case("content-type"))
            {
                headers.push_str("Content-Type: ");
                headers.push_str(content_type);
                headers.push_str("\r\n");
            }
        }
        PreparedBody::None => {}
    }

    headers
}

/// Prepares headers string from request additional_headers only (no streaming).
#[cfg(not(any(feature = "blocking-stream", feature = "async-stream")))]
pub(crate) fn prepare_additional_headers(
    additional_headers: &[(Cow<'static, str>, Cow<'static, str>)],
    options: &nyquest_interface::client::ClientOptions,
    body: &PreparedBody,
) -> String {
    let mut headers = String::new();

    // Add default headers
    for (name, value) in &options.default_headers {
        // Skip if overridden in additional_headers
        if !additional_headers
            .iter()
            .any(|(n, _)| n.eq_ignore_ascii_case(name))
        {
            headers.push_str(name);
            headers.push_str(": ");
            headers.push_str(value);
            headers.push_str("\r\n");
        }
    }

    // Add request-specific headers
    for (name, value) in additional_headers {
        headers.push_str(name);
        headers.push_str(": ");
        headers.push_str(value);
        headers.push_str("\r\n");
    }

    headers
}

/// Prepares the request body (with streaming support).
#[cfg(any(feature = "blocking-stream", feature = "async-stream"))]
pub(crate) fn prepare_body<S>(
    body: Option<Body<S>>,
    headers: &mut String,
    get_stream_len: impl Fn(&S) -> Option<u64>,
) -> PreparedBody<S> {
    use crate::stream::DataOrStream;

    match body {
        None => PreparedBody::None,
        Some(Body::Bytes {
            content,
            content_type,
        }) => {
            headers.push_str("Content-Type: ");
            headers.push_str(&content_type);
            headers.push_str("\r\n");
            PreparedBody::Complete(content.into_owned())
        }
        Some(Body::Form { fields }) => {
            headers.push_str("Content-Type: application/x-www-form-urlencoded\r\n");
            let encoded = encode_form_fields(&fields);
            PreparedBody::Complete(encoded.into_bytes())
        }
        #[cfg(feature = "multipart")]
        Some(Body::Multipart { parts }) => {
            let boundary = crate::multipart::generate_multipart_boundary();
            headers.push_str("Content-Type: multipart/form-data; boundary=");
            headers.push_str(&boundary);
            headers.push_str("\r\n");
            let body_parts = crate::multipart::generate_multipart_body(&boundary, parts);

            // Check if there are any streams - if not, collect to complete body
            let has_streams = body_parts
                .iter()
                .any(|p| matches!(p, DataOrStream::Stream(_)));
            if !has_streams {
                // All data parts, collect into single Vec
                let data: Vec<u8> = body_parts
                    .into_iter()
                    .flat_map(|p| match p {
                        DataOrStream::Data(d) => d,
                        DataOrStream::Stream(_) => unreachable!(),
                    })
                    .collect();
                PreparedBody::Complete(data)
            } else {
                // Has streams, use streaming upload
                PreparedBody::Stream {
                    content_type: format!("multipart/form-data; boundary={}", boundary),
                    stream_parts: body_parts,
                }
            }
        }
        Some(Body::Stream {
            stream,
            content_type,
        }) => {
            // Check if stream has known length
            let content_length = get_stream_len(&stream);
            if let Some(len) = content_length {
                headers.push_str("Content-Length: ");
                headers.push_str(&len.to_string());
                headers.push_str("\r\n");
            }

            PreparedBody::Stream {
                content_type: content_type.into_owned(),
                stream_parts: vec![DataOrStream::Stream(stream)],
            }
        }
    }
}

/// Prepares the request body (without streaming support).
#[cfg(not(any(feature = "blocking-stream", feature = "async-stream")))]
pub(crate) fn prepare_body<S>(
    body: Option<Body<S>>,
    headers: &mut String,
    _get_stream_len: impl Fn(&S) -> Option<u64>,
) -> PreparedBody {
    match body {
        None => PreparedBody::None,
        Some(Body::Bytes {
            content,
            content_type,
        }) => {
            headers.push_str("Content-Type: ");
            headers.push_str(&content_type);
            headers.push_str("\r\n");
            PreparedBody::Complete(content.into_owned())
        }
        Some(Body::Form { fields }) => {
            headers.push_str("Content-Type: application/x-www-form-urlencoded\r\n");
            let encoded = encode_form_fields(&fields);
            PreparedBody::Complete(encoded.into_bytes())
        }
        #[cfg(feature = "multipart")]
        Some(Body::Multipart { parts }) => {
            let boundary = crate::multipart::generate_multipart_boundary();
            headers.push_str("Content-Type: multipart/form-data; boundary=");
            headers.push_str(&boundary);
            headers.push_str("\r\n");
            // Without stream feature, use non-streaming multipart generation
            let body_data = crate::multipart::generate_multipart_body_bytes(&boundary, parts);
            PreparedBody::Complete(body_data)
        }
        Some(Body::Stream { .. }) => {
            unreachable!("streaming requires stream feature")
        }
    }
}

/// URL-encodes form fields.
fn encode_form_fields(fields: &[(Cow<'static, str>, Cow<'static, str>)]) -> String {
    let mut result = String::new();
    for (i, (key, value)) in fields.iter().enumerate() {
        if i > 0 {
            result.push('&');
        }
        result.push_str(&url_encode(key));
        result.push('=');
        result.push_str(&url_encode(value));
    }
    result
}

/// Simple URL encoding for form data (application/x-www-form-urlencoded).
/// Spaces are encoded as '+' per the application/x-www-form-urlencoded format.
fn url_encode(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            'A'..='Z' | 'a'..='z' | '0'..='9' | '-' | '_' | '.' | '~' => result.push(c),
            ' ' => result.push('+'),
            _ => {
                for b in c.to_string().as_bytes() {
                    result.push_str(&format!("%{:02X}", b));
                }
            }
        }
    }
    result
}

/// Converts nyquest Method to HTTP method string.
pub(crate) fn method_to_cwstr(method: &Method) -> Cow<'static, [u16]> {
    match method {
        Method::Get => u16cstr!("GET"),
        Method::Post => u16cstr!("POST"),
        Method::Put => u16cstr!("PUT"),
        Method::Delete => u16cstr!("DELETE"),
        Method::Patch => u16cstr!("PATCH"),
        Method::Head => u16cstr!("HEAD"),
        Method::Other(m) => return m.encode_utf16().chain(std::iter::once(0)).collect(),
    }
    .as_slice_with_nul()
    .into()
}

/// Creates connection and request handles for the given URL.
pub(crate) fn create_request(
    session: &WinHttpSession,
    parsed_url: &ParsedUrl,
    method_cwstr: &[u16],
) -> Result<(ConnectionHandle, RequestHandle)> {
    let connection = ConnectionHandle::connect(&session.session, parsed_url.host, parsed_url.port)?;
    let request = RequestHandle::open(
        &connection,
        method_cwstr,
        &parsed_url.path,
        parsed_url.is_secure,
    )?;

    // Apply per-request options
    if session.options.ignore_certificate_errors && parsed_url.is_secure {
        request.ignore_certificate_errors()?;
    }

    if !session.options.use_cookies {
        request.disable_cookies()?;
    }

    if !session.options.follow_redirects {
        request.disable_redirects()?;
    }

    // Set receive response timeout at the request level for more reliable timeout behavior
    if let Some(timeout) = session.options.request_timeout {
        let timeout_ms = timeout.as_millis() as u32;
        request.set_receive_response_timeout(timeout_ms)?;
    }

    Ok((connection, request))
}
