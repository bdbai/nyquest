//! Blocking WinHTTP client implementation.

use std::sync::Arc;

use nyquest_interface::blocking::{BlockingBackend, BlockingClient, Request};
use nyquest_interface::client::ClientOptions;
use nyquest_interface::Result as NyquestResult;

use super::response::WinHttpBlockingResponse;
use crate::error::WinHttpResultExt;
#[cfg(feature = "blocking-progress")]
use crate::progress::WinHttpProgressTracker;
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

    fn request_single(
        &self,
        req: Request,
        #[cfg(feature = "blocking-progress")] progress: Option<
            std::sync::Arc<dyn nyquest_interface::ProgressReporter>,
        >,
    ) -> NyquestResult<WinHttpBlockingResponse> {
        // Create connection and request handles
        let url = concat_url(self.session.base_cwurl.as_deref(), &req.relative_uri)?;
        let (connection, request) = {
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

        #[cfg(feature = "blocking-progress")]
        let progress_tracker = progress.map(|reporter| {
            // A zero or unknown length means the total is unknown.
            let upload_total = body_len.and_then(crate::progress::normalize_total);
            WinHttpProgressTracker::new(reporter, upload_total, None)
        });

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
                #[cfg(feature = "blocking-progress")]
                if let Some(tracker) = &progress_tracker {
                    tracker.add_uploaded(data.len() as u64);
                }
            },
            #[cfg(feature = "blocking-stream")]
            PreparedBody::Stream { stream_parts, .. } => {
                let uploaded = self.send_streaming_request(&request, stream_parts, body_len)?;
                #[cfg(feature = "blocking-progress")]
                if let Some(tracker) = &progress_tracker {
                    tracker.add_uploaded(uploaded);
                }
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

        #[cfg(feature = "blocking-progress")]
        if let Some(tracker) = &progress_tracker {
            tracker.set_download_total(content_length);
        }

        // `mut` is only needed when the progress tracker is attached below.
        #[allow(unused_mut)]
        let mut response = WinHttpBlockingResponse::new(
            self.session.clone(),
            connection,
            request,
            status,
            content_length,
            self.session.options.max_response_buffer_size,
        );
        #[cfg(feature = "blocking-progress")]
        {
            response.progress = progress_tracker;
        }
        Ok(response)
    }
}

impl BlockingClient for WinHttpBlockingClient {
    type Response = WinHttpBlockingResponse;

    fn request(&self, req: Request) -> NyquestResult<Self::Response> {
        #[cfg(feature = "blocking-progress")]
        return self.request_single(req, None);
        #[cfg(not(feature = "blocking-progress"))]
        return self.request_single(req);
    }

    #[cfg(feature = "blocking-progress")]
    fn request_with_progress(
        &self,
        req: Request,
        progress: Arc<dyn nyquest_interface::ProgressReporter>,
    ) -> NyquestResult<Self::Response> {
        self.request_single(req, Some(progress))
    }

    #[cfg(feature = "blocking-ws")]
    fn connect_websocket(
        &self,
        uri: std::borrow::Cow<'static, str>,
        protocols: Vec<std::borrow::Cow<'static, str>>,
    ) -> nyquest_interface::websocket::WebSocketResult<
        nyquest_interface::blocking::websocket::WebSocket,
    > {
        super::websocket::connect_blocking(&self.session, uri, protocols)
    }

    #[cfg(feature = "cookies")]
    fn add_cookie(
        &self,
        _name: std::borrow::Cow<'static, str>,
        _value: std::borrow::Cow<'static, str>,
        _url: std::borrow::Cow<'static, str>,
    ) -> NyquestResult<()> {
        // WinHTTP manages cookies internally per session and does not expose
        // an API to enumerate or modify its cookie cache. Automatic cookie
        // handling (controlled by `ClientOptions::use_cookies`) keeps working;
        // only explicit management is unsupported.
        Err(nyquest_interface::Error::Io(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            "explicit cookie management is not supported by the WinHTTP backend; \
             automatic cookies remain controlled by `use_cookies`",
        )))
    }

    #[cfg(feature = "cookies")]
    fn get_cookies_for(
        &self,
        _url: std::borrow::Cow<'static, str>,
    ) -> NyquestResult<Vec<nyquest_interface::Cookie>> {
        Err(nyquest_interface::Error::Io(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            "explicit cookie management is not supported by the WinHTTP backend; \
             automatic cookies remain controlled by `use_cookies`",
        )))
    }
}

#[cfg(feature = "blocking-stream")]
impl WinHttpBlockingClient {
    fn send_streaming_request(
        &self,
        request: &crate::handle::RequestHandle,
        stream_parts: Vec<DataOrStream<BoxedStream>>,
        content_length: Option<u64>,
    ) -> NyquestResult<u64> {
        use crate::error::WinHttpResultExt as _;

        let mut writer = if let Some(len) = content_length {
            request.send_with_total_length(len, 0).into_nyquest()?;
            StreamWriter::new(stream_parts, false)
        } else {
            request.send_chunked(0).into_nyquest()?;
            StreamWriter::new(stream_parts, true)
        };

        let mut total_written = 0u64;
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
                total_written += written as u64;
            }
            writer.advance(buf);
        }

        Ok(total_written)
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
