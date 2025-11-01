use std::io::{self, Read as _};

use curl::easy::ReadError;
use nyquest_interface::blocking::BoxedStream;

use crate::curl_ng::mime::MimePartReader;

pub(super) struct BlockingPartReader {
    stream: BoxedStream,
}

impl BlockingPartReader {
    pub fn new(stream: BoxedStream) -> Self {
        Self { stream }
    }
}

impl MimePartReader for BlockingPartReader {
    fn read(&mut self, buf: &mut [u8]) -> Result<usize, ReadError> {
        // FIXME: propagate IO errors
        self.stream.read(buf).map_err(|_| ReadError::Abort)
    }

    fn seek(&mut self, whence: io::SeekFrom) -> curl::easy::SeekResult {
        let BoxedStream::Sized { stream, .. } = &mut self.stream else {
            return curl::easy::SeekResult::CantSeek;
        };
        match stream.seek(whence) {
            Ok(_pos) => curl::easy::SeekResult::Ok,
            Err(_) => curl::easy::SeekResult::Fail,
        }
    }
}
