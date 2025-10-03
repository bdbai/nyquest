use curl::easy::WriteError;

use crate::{curl_ng::easy::EasyCallback, state::RequestState};
use nyquest_interface::{blocking::BoxedStream, SizedStream};

#[derive(Debug, Default)]
pub struct BlockingHandler {
    pub(super) state: RequestState,
    body_stream: Option<SizedStream<BoxedStream>>,
}

impl BlockingHandler {
    pub fn new(state: RequestState) -> Self {
        Self {
            state,
            body_stream: None,
        }
    }

    pub fn set_body_stream(&mut self, body_stream: SizedStream<BoxedStream>) {
        self.body_stream = Some(body_stream);
    }
}

impl EasyCallback for BlockingHandler {
    fn write(&mut self, data: &[u8]) -> Result<usize, WriteError> {
        self.state.write_data(data);
        Ok(data.len())
    }

    fn header(&mut self, data: &[u8]) -> bool {
        self.state.push_header_data(data);
        true
    }

    fn read(&mut self, _data: &mut [u8]) -> Result<usize, curl::easy::ReadError> {
        let Some(stream) = &mut self.body_stream else {
            return Ok(0);
        };
        match stream.stream.read(data) {
            Ok(n) => Ok(n),
            // FIXME: propagate IO errors
            Err(_e) => Err(curl::easy::ReadError::Abort),
        }
    }

    fn seek(&mut self, whence: std::io::SeekFrom) -> curl::easy::SeekResult {
        let Some(stream) = &mut self.body_stream else {
            return curl::easy::SeekResult::Fail;
        };
        match stream.seek(whence) {
            Ok(_pos) => curl::easy::SeekResult::Ok,
            // FIXME: propagate IO errors
            Err(_e) => curl::easy::SeekResult::Fail,
        }
    }
}
