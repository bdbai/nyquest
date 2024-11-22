use std::io;

use windows::core::HSTRING;
use windows::Storage::Streams::{DataReader, InputStreamOptions};
use windows::Web::Http::{HttpResponseMessage, IHttpContent};

pub struct WinrtResponse {
    pub(crate) status: u16,
    pub(crate) content_length: Option<u64>,
    pub(crate) response: HttpResponseMessage,
    pub(crate) reader: Option<DataReader>,
}

impl WinrtResponse {
    pub(crate) fn new(res: HttpResponseMessage) -> io::Result<WinrtResponse> {
        let content_length = match res.Content() {
            Ok(content) => content.Headers()?.ContentLength()?.Value().ok(),
            Err(_) => Some(0),
        };
        Ok(WinrtResponse {
            status: res.StatusCode()?.0 as u16,
            content_length,
            response: res,
            reader: None,
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

    pub(crate) fn reader_mut(&mut self) -> io::Result<&mut DataReader> {
        if self.reader.is_none() {
            let content = self.content()?;
            let content = content.ReadAsInputStreamAsync()?.get()?;
            let reader = DataReader::CreateDataReader(&content)?;
            reader.SetInputStreamOptions(InputStreamOptions::Partial)?;
            self.reader = Some(reader);
        }
        Ok(self.reader.as_mut().expect("DataReader is None"))
    }
}
