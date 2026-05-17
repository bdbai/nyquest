use std::io;

use windows::core::HSTRING;
use windows::Web::Http::{HttpResponseMessage, IHttpContent};

use crate::timer::Timer;

pub struct WinrtResponse {
    pub(crate) status: u16,
    pub(crate) content_length: Option<u64>,
    pub(crate) max_response_buffer_size: Option<u64>,
    pub(crate) request_timer: Timer,
    pub(crate) response: HttpResponseMessage,
}

impl WinrtResponse {
    pub(crate) fn new(
        res: HttpResponseMessage,
        response_size_limit: Option<u64>,
        request_timer: Timer,
    ) -> io::Result<WinrtResponse> {
        let content_length = match res.Content() {
            Ok(content) => content
                .Headers()?
                .ContentLength()
                .ok()
                .and_then(|v| v.Value().ok()),
            Err(_) => Some(0),
        };
        Ok(WinrtResponse {
            status: res.StatusCode()?.0 as u16,
            content_length,
            max_response_buffer_size: response_size_limit,
            request_timer,
            response: res,
        })
    }

    pub(crate) fn get_header(&self, header: &str) -> io::Result<Vec<String>> {
        let headers = self.response.Headers()?;
        let header_name = HSTRING::from(header);
        let mut headers = headers.Lookup(&header_name).ok();
        if headers.is_none() {
            headers = self.content()?.Headers()?.Lookup(&header_name).ok();
        }
        Ok(headers.into_iter().map(|h| h.to_string_lossy()).collect())
    }

    pub(crate) fn content(&self) -> io::Result<IHttpContent> {
        Ok(self.response.Content()?)
    }
}
