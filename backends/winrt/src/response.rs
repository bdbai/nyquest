use std::io;

use windows::Storage::Streams::{DataReader, InputStreamOptions};
use windows::Web::Http::{HttpResponseMessage, IHttpContent};

pub struct WinrtResponse {
    pub(crate) response: HttpResponseMessage,
    pub(crate) reader: Option<DataReader>,
}

impl WinrtResponse {
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
