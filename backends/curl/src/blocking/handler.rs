use curl::easy::WriteError;

use crate::{curl_ng::easy::EasyCallback, state::RequestState};

#[derive(Debug, Default)]
pub struct BlockingHandler {
    pub(super) state: RequestState,
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
        Ok(0)
    }

    fn seek(&mut self, _whence: std::io::SeekFrom) -> curl::easy::SeekResult {
        curl::easy::SeekResult::Fail
    }
}
