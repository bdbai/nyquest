//! Blocking WinHTTP client implementation.

use std::sync::Arc;

use nyquest_interface::blocking::{BlockingBackend, BlockingClient, Request};
use nyquest_interface::client::ClientOptions;
use nyquest_interface::Result as NyquestResult;

use super::response::WinHttpBlockingResponse;
use crate::error::WinHttpResultExt;
use crate::request::{
    create_request, method_to_cwstr, prepare_additional_headers, prepare_body, PreparedBody,
};
use crate::session::WinHttpSession;
use crate::url::{concat_url, ParsedUrl};
use crate::WinHttpBackend;

#[cfg(feature = "blocking-stream")]
use crate::stream::{DataOrStream, StreamWriter};
#[cfg(feature = "blocking-stream")]
use nyquest_interface::blocking::BoxedStream;

/// Blocking WinHTTP client.
#[derive(Clone)]
pub struct WinHttpBlockingClient {
    session: Arc<WinHttpSession>,
}

impl WinHttpBlockingClient {
    pub(crate) fn new(options: ClientOptions) -> NyquestResult<Self> {
        let session = WinHttpSession::new(options, false).into_nyquest()?;
        Ok(Self { session })
    }
}

impl BlockingClient for WinHttpBlockingClient {
    type Response = WinHttpBlockingResponse;

    fn request(&self, req: Request) -> NyquestResult<Self::Response> {
        // Create connection and request handles
        let (connection, request) = {
            let url = concat_url(self.session.base_cwurl.as_deref(), &req.relative_uri);
            let parsed_url = ParsedUrl::parse(&url).ok_or(nyquest_interface::Error::InvalidUrl)?;
            let method = method_to_cwstr(&req.method);
            create_request(&self.session, &parsed_url, &method).into_nyquest()?
        };

        // Prepare headers and body
        let prepared_body = prepare_body(req.body, get_stream_content_length);
        let headers_str = prepare_additional_headers(
            &req.additional_headers,
            &self.session.options,
            &prepared_body,
        );

        let body_len = prepared_body.body_len();

        // Add headers
        if !headers_str.is_empty() {
            request.add_headers(&headers_str).into_nyquest()?;
        }

        // Send the request
        match prepared_body {
            PreparedBody::None => unsafe {
                request.send(std::ptr::null(), 0, 0).into_nyquest()?;
            },
            // SAFETY: Since the request handle is in blocking mode, WinHTTP
            // will not read the body data until send() returns.
            PreparedBody::Complete { data, .. } => unsafe {
                request.send(data.as_ptr(), data.len(), 0).into_nyquest()?;
            },
            #[cfg(feature = "blocking-stream")]
            PreparedBody::Stream { stream_parts, .. } => {
                self.send_streaming_request(&request, stream_parts, body_len)?;
            }
            #[cfg(not(feature = "blocking-stream"))]
            PreparedBody::Stream { .. } => {
                unreachable!("streaming requires blocking-stream feature")
            }
        }

        // Receive response
        request.receive_response().into_nyquest()?;

        // Get status code
        let status = request.query_status_code().into_nyquest()?;
        let content_length = request.query_content_length();

        Ok(WinHttpBlockingResponse::new(
            self.session.clone(),
            connection,
            request,
            status,
            content_length,
            self.session.options.max_response_buffer_size,
        ))
    }
}

#[cfg(feature = "blocking-stream")]
impl WinHttpBlockingClient {
    fn send_streaming_request(
        &self,
        request: &crate::handle::RequestHandle,
        stream_parts: Vec<DataOrStream<BoxedStream>>,
        content_length: Option<u64>,
    ) -> NyquestResult<()> {
        use crate::error::WinHttpResultExt as _;

        let mut writer = if let Some(len) = content_length {
            request.send_with_total_length(len, 0).into_nyquest()?;
            StreamWriter::new(stream_parts, false)
        } else {
            request.send_chunked(0).into_nyquest()?;
            StreamWriter::new(stream_parts, true)
        };

        while !writer.is_finished() {
            use std::io::Read as _;
            use std::task::Poll;

            let (buf, mut range) = match writer
                .poll_take_buffer(|stream, buf| Poll::Ready(stream.read(buf)))
            {
                Poll::Ready(Ok(res)) => res,
                Poll::Ready(Err(e)) => return Err(e.into()),
                Poll::Pending => {
                    unreachable!("poll_take_buffer should never return Pending in blocking mode")
                }
            };
            while !range.is_empty() {
                let data = &buf[range.start..range.end];
                let written = unsafe { request.write_data(data).into_nyquest()? };
                range.start += written as usize;
            }
            writer.advance(buf);
        }

        Ok(())
    }
}

/// Extracts content length from a BoxedStream if it's a sized stream.
#[cfg(feature = "blocking-stream")]
fn get_stream_content_length(stream: &BoxedStream) -> Option<u64> {
    match stream {
        BoxedStream::Sized { content_length, .. } => Some(*content_length),
        BoxedStream::Unsized { .. } => None,
    }
}

#[cfg(not(feature = "blocking-stream"))]
fn get_stream_content_length(_stream: &impl Sized) -> Option<u64> {
    None
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
