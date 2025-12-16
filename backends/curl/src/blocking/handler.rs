use curl::easy::{ReadError, SeekResult, WriteError};

use crate::{curl_ng::easy::EasyCallback, state::RequestState};
#[cfg(feature = "blocking-stream")]
use nyquest_interface::blocking::BoxedStream;

#[derive(Default)]
pub struct BlockingHandler {
    pub(super) state: RequestState,
    #[cfg(feature = "blocking-stream")]
    body_stream: Option<BoxedStream>,
}

impl BlockingHandler {
    #[cfg(feature = "blocking-stream")]
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

    #[cfg(feature = "blocking-stream")]
    fn read(&mut self, data: &mut [u8]) -> Result<usize, ReadError> {
        use std::io::Read as _;

        let Some(stream) = &mut self.body_stream else {
            return Ok(0);
        };
        match stream.read(data) {
            Ok(n) => Ok(n),
            // FIXME: propagate IO errors
            Err(_e) => Err(ReadError::Abort),
        }
    }

    #[cfg(feature = "blocking-stream")]
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

    #[cfg(not(feature = "blocking-stream"))]
    fn read(&mut self, _data: &mut [u8]) -> Result<usize, ReadError> {
        Err(ReadError::Abort)
    }

    #[cfg(not(feature = "blocking-stream"))]
    fn seek(&mut self, _whence: std::io::SeekFrom) -> SeekResult {
        SeekResult::Fail
    }
}
