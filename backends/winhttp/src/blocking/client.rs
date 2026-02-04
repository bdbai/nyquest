//! Blocking WinHTTP client implementation.

use std::sync::Arc;

use nyquest_interface::blocking::{BlockingBackend, BlockingClient, Request};
use nyquest_interface::client::ClientOptions;
use nyquest_interface::Result as NyquestResult;

use super::response::WinHttpBlockingResponse;
use crate::error::WinHttpResultExt;
use crate::handle::RequestHandle;
use crate::request::{
    create_request, method_to_str, prepare_additional_headers, prepare_body, PreparedBody,
};
use crate::session::WinHttpSession;
use crate::url::{concat_url, ParsedUrl};
use crate::WinHttpBackend;

#[cfg(feature = "blocking-stream")]
use nyquest_interface::blocking::BoxedStream;

/// Blocking WinHTTP client.
#[derive(Clone)]
pub struct WinHttpBlockingClient {
    session: Arc<WinHttpSession>,
}

impl WinHttpBlockingClient {
    pub(crate) fn new(options: ClientOptions) -> NyquestResult<Self> {
        let session = WinHttpSession::new_blocking(options).into_nyquest()?;
        Ok(Self { session })
    }

    /// Extracts content length from a BoxedStream if it's a sized stream.
    #[cfg(feature = "blocking-stream")]
    fn get_stream_content_length(stream: &BoxedStream) -> Option<u64> {
        match stream {
            BoxedStream::Sized { content_length, .. } => Some(*content_length),
            BoxedStream::Unsized { .. } => None,
        }
    }
}

impl BlockingClient for WinHttpBlockingClient {
    type Response = WinHttpBlockingResponse;

    fn request(&self, req: Request) -> NyquestResult<Self::Response> {
        // Parse the URL
        let url = concat_url(self.session.options.base_url.as_deref(), &req.relative_uri);
        let parsed_url = ParsedUrl::parse(&url).ok_or(nyquest_interface::Error::InvalidUrl)?;

        let method = method_to_str(&req.method);

        // Create connection and request handles
        let (connection, request) =
            create_request(&self.session, &parsed_url, method).into_nyquest()?;

        // Store additional headers before consuming body
        let additional_headers = req.additional_headers.clone();

        // Prepare headers and body
        let mut headers_str = String::new();
        let (prepared_body, _stream) = prepare_body(req.body, &mut headers_str);
        headers_str.push_str(&prepare_additional_headers(
            &additional_headers,
            &self.session.options,
            &prepared_body,
        ));

        // For unsized streams, add Transfer-Encoding: chunked header
        #[cfg(feature = "blocking-stream")]
        let content_length = _stream.as_ref().and_then(Self::get_stream_content_length);
        #[cfg(feature = "blocking-stream")]
        if matches!(&prepared_body, PreparedBody::Stream { .. }) && content_length.is_none() {
            headers_str.push_str("Transfer-Encoding: chunked\r\n");
        }

        // Add headers
        if !headers_str.is_empty() {
            request.add_headers(&headers_str).into_nyquest()?;
        }

        // Send the request
        match prepared_body {
            PreparedBody::None => {
                request.send(None).into_nyquest()?;
            }
            PreparedBody::Complete(data) => {
                request.send(Some(&data)).into_nyquest()?;
            }
            #[cfg(feature = "blocking-stream")]
            PreparedBody::Stream { .. } => {
                // For streaming uploads, use the already-extracted content length
                self.send_streaming_request(&request, _stream, content_length)?;
            }
        }

        // Receive response
        request.receive_response().into_nyquest()?;

        // Get status code
        let status = request.query_status_code().into_nyquest()?;
        let content_length = request.query_content_length();

        // Parse headers
        let headers = parse_response_headers(&request)?;

        Ok(WinHttpBlockingResponse::new(
            connection,
            request,
            status,
            content_length,
            headers,
            self.session.max_response_buffer_size(),
        ))
    }
}

#[cfg(feature = "blocking-stream")]
impl WinHttpBlockingClient {
    fn send_streaming_request<S: std::io::Read>(
        &self,
        request: &RequestHandle,
        stream: Option<S>,
        content_length: Option<u64>,
    ) -> NyquestResult<()> {
        use crate::error::WinHttpResultExt;

        let Some(mut stream) = stream else {
            request.send(None).into_nyquest()?;
            return Ok(());
        };

        // Send the request with appropriate content length handling
        if let Some(len) = content_length {
            // For sized streams, use the known content length
            request.send_with_total_length(len).into_nyquest()?;

            // Write data in chunks (no chunked encoding, just raw data)
            let mut buffer = [0u8; 8192];
            loop {
                let bytes_read = stream.read(&mut buffer)?;
                if bytes_read == 0 {
                    break;
                }
                request.write_data(&buffer[..bytes_read]).into_nyquest()?;
            }
        } else {
            // For streaming uploads with unknown content length, we use chunked encoding.
            // When Transfer-Encoding: chunked is set, we must format the data ourselves
            // in the chunked transfer encoding format.
            request.send_for_streaming().into_nyquest()?;

            // Write data in chunks using HTTP chunked transfer encoding format
            let mut buffer = [0u8; 8192];
            loop {
                let bytes_read = stream.read(&mut buffer)?;
                if bytes_read == 0 {
                    break;
                }
                // Write chunk: <size in hex>\r\n<data>\r\n
                let header = format!("{:X}\r\n", bytes_read);
                request.write_data(header.as_bytes()).into_nyquest()?;
                request.write_data(&buffer[..bytes_read]).into_nyquest()?;
                request.write_data(b"\r\n").into_nyquest()?;
            }

            // Write final chunk (terminator): 0\r\n\r\n
            request.write_data(b"0\r\n\r\n").into_nyquest()?;
        }

        Ok(())
    }
}

fn parse_response_headers(request: &RequestHandle) -> NyquestResult<Vec<(String, String)>> {
    let raw_headers = request.query_raw_headers().into_nyquest()?;
    let mut headers = Vec::new();

    for line in raw_headers.lines() {
        if line.is_empty() {
            continue;
        }
        // Skip status line
        if line.starts_with("HTTP/") {
            continue;
        }
        if let Some((name, value)) = line.split_once(':') {
            headers.push((name.trim().to_string(), value.trim().to_string()));
        }
    }

    Ok(headers)
}

impl BlockingBackend for WinHttpBackend {
    type BlockingClient = WinHttpBlockingClient;

    fn create_blocking_client(
        &self,
        options: ClientOptions,
    ) -> NyquestResult<Self::BlockingClient> {
        WinHttpBlockingClient::new(options)
    }
}
