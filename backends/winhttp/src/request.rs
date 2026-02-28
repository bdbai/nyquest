//! Request building utilities for WinHTTP backend.

use std::borrow::Cow;

use nyquest_interface::{Body, Method};
use widestring::u16cstr;

use crate::error::Result;
use crate::handle::{ConnectionHandle, RequestHandle};
use crate::session::WinHttpSession;
use crate::url::ParsedUrl;

/// Prepared request body data.
pub(crate) enum PreparedBody<S> {
    /// No body
    None,
    /// Complete body data
    Complete {
        content_type: Cow<'static, str>,
        data: Cow<'static, [u8]>,
    },
    /// Streaming body with content type and optional content length
    Stream {
        content_type: Cow<'static, str>,
        content_length: Option<u64>,
        stream_parts: Vec<crate::stream::DataOrStream<S>>,
    },
}

impl<S> PreparedBody<S> {
    pub(crate) fn body_len(&self) -> Option<u64> {
        match self {
            PreparedBody::None => Some(0),
            PreparedBody::Complete { data, .. } => Some(data.len() as u64),
            PreparedBody::Stream { content_length, .. } => *content_length,
        }
    }

    pub(crate) fn take_body(&mut self) -> Option<Cow<'static, [u8]>> {
        if let PreparedBody::Complete { data, .. } = self {
            Some(std::mem::take(data))
        } else {
            None
        }
    }
}

/// Prepares headers string from request additional_headers only.
pub(crate) fn prepare_additional_headers<S>(
    additional_headers: &[(Cow<'static, str>, Cow<'static, str>)],
    options: &nyquest_interface::client::ClientOptions,
    body: &PreparedBody<S>,
) -> String {
    let mut headers = String::new();

    // Add request-specific headers
    for (name, value) in additional_headers {
        headers.push_str(name);
        headers.push_str(": ");
        headers.push_str(value);
        headers.push_str("\r\n");
    }

    // Add Content-Type if needed
    match body {
        PreparedBody::Complete { content_type, .. } => {
            if !additional_headers
                .iter()
                .any(|(n, _)| n.eq_ignore_ascii_case("content-type"))
            {
                headers.push_str("Content-Type: ");
                headers.push_str(content_type);
                headers.push_str("\r\n");
            }
        }
        PreparedBody::Stream {
            content_type,
            content_length,
            ..
        } => {
            if !additional_headers
                .iter()
                .any(|(n, _)| n.eq_ignore_ascii_case("content-type"))
            {
                headers.push_str("Content-Type: ");
                headers.push_str(content_type);
                headers.push_str("\r\n");
            }
            if let Some(len) = content_length {
                if !additional_headers
                    .iter()
                    .any(|(n, _)| n.eq_ignore_ascii_case("content-length"))
                {
                    headers.push_str("Content-Length: ");
                    headers.push_str(&len.to_string());
                    headers.push_str("\r\n");
                }
            } else {
                // No content length, use chunked encoding
                if !additional_headers
                    .iter()
                    .any(|(n, _)| n.eq_ignore_ascii_case("transfer-encoding"))
                {
                    headers.push_str("Transfer-Encoding: chunked\r\n");
                }
            }
        }
        PreparedBody::None => {}
    }

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

    headers
}

/// Prepares the request body.
pub(crate) fn prepare_body<S>(
    body: Option<Body<S>>,
    get_stream_len: impl Fn(&S) -> Option<u64>,
) -> PreparedBody<S> {
    use crate::stream::DataOrStream;

    match body {
        None => PreparedBody::None,
        Some(Body::Bytes {
            content,
            content_type,
        }) => {
            if u32::try_from(content.len()).is_ok() {
                PreparedBody::Complete {
                    content_type,
                    data: content,
                }
            } else {
                let max_chunk_size = u32::MAX as usize;
                let mut parts = Vec::with_capacity(content.len() / u32::MAX as usize + 1);
                let mut offset = 0;
                while offset < content.len() {
                    let chunk_size = max_chunk_size.min(content.len() - offset);
                    parts.push(DataOrStream::Data(content[offset..][..chunk_size].to_vec()));
                    offset += chunk_size;
                }
                PreparedBody::Stream {
                    content_type,
                    content_length: Some(content.len() as u64),
                    stream_parts: parts,
                }
            }
        }
        Some(Body::Form { fields }) => {
            let encoded = encode_form_fields(&fields);
            PreparedBody::Complete {
                content_type: "application/x-www-form-urlencoded".into(),
                data: Cow::Owned(encoded.into_bytes()),
            }
        }
        #[cfg(feature = "multipart")]
        Some(Body::Multipart { parts }) => {
            let boundary = crate::multipart::generate_multipart_boundary();
            let body_parts = crate::multipart::generate_multipart_body(&boundary, parts);
            let content_type = format!("multipart/form-data; boundary={}", boundary).into();

            // Check if there are any streams - if not, collect to complete body
            let mut body_parts_it = body_parts.into_iter();
            let mut first_data_part = vec![];
            let first_stream = 'only_data: {
                for part in body_parts_it.by_ref() {
                    match part {
                        DataOrStream::Data(data) => {
                            if first_data_part.is_empty() {
                                first_data_part = data;
                            } else {
                                first_data_part.extend_from_slice(&data);
                            }
                        }
                        DataOrStream::Stream(s) => {
                            break 'only_data s;
                        }
                    }
                }
                return PreparedBody::Complete {
                    content_type,
                    data: Cow::Owned(first_data_part),
                };
            };

            // Has streams, use streaming upload
            let parts: Vec<_> = [
                DataOrStream::Data(first_data_part),
                DataOrStream::Stream(first_stream),
            ]
            .into_iter()
            .chain(body_parts_it)
            .collect();
            let content_length = parts
                .iter()
                .map(|p| match p {
                    DataOrStream::Data(data) => Some(data.len() as u64),
                    DataOrStream::Stream(s) => get_stream_len(s),
                })
                .sum();
            PreparedBody::Stream {
                content_type,
                content_length,
                stream_parts: parts,
            }
        }
        #[cfg(any(feature = "blocking-stream", feature = "async-stream"))]
        Some(Body::Stream {
            stream,
            content_type,
        }) => {
            let content_length = get_stream_len(&stream);

            PreparedBody::Stream {
                content_type,
                content_length,
                stream_parts: vec![DataOrStream::Stream(stream)],
            }
        }
        #[cfg(not(any(feature = "blocking-stream", feature = "async-stream")))]
        Some(Body::Stream { .. }) => {
            unreachable!("streaming requires stream feature")
        }
    }
}

/// URL-encodes form fields.
fn encode_form_fields(fields: &[(Cow<'static, str>, Cow<'static, str>)]) -> String {
    let result = String::with_capacity(fields.iter().map(|(k, v)| k.len() + v.len() + 2).sum());
    form_urlencoded::Serializer::new(result)
        .extend_pairs(fields)
        .finish()
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
        parsed_url.path_and_query,
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
