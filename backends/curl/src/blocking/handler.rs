use std::io::Read as _;

use curl::easy::{ReadError, SeekResult, WriteError};

use crate::{curl_ng::easy::EasyCallback, state::RequestState};
use nyquest_interface::blocking::BoxedStream;

#[derive(Default)]
pub struct BlockingHandler {
    pub(super) state: RequestState,
    body_stream: Option<BoxedStream>,
}

impl BlockingHandler {
    pub fn set_body_stream(&mut self, body_stream: BoxedStream) {
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

    fn read(&mut self, data: &mut [u8]) -> Result<usize, ReadError> {
        let Some(stream) = &mut self.body_stream else {
            return Ok(0);
        };
        match stream.read(data) {
            Ok(n) => Ok(n),
            // FIXME: propagate IO errors
            Err(_e) => Err(ReadError::Abort),
        }
    }

    fn seek(&mut self, whence: std::io::SeekFrom) -> SeekResult {
        let Some(stream) = &mut self.body_stream else {
            return SeekResult::Fail;
        };
        let BoxedStream::Sized { stream, .. } = stream else {
            return SeekResult::CantSeek;
        };
        match stream.seek(whence) {
            Ok(_pos) => SeekResult::Ok,
            // FIXME: propagate IO errors
            Err(_e) => SeekResult::Fail,
        }
    }
}
