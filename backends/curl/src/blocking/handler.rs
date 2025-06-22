use std::sync::{Arc, Mutex};

use curl::easy::{Handler, WriteError};

use crate::state::RequestState;

pub struct BlockingHandler {
    state: Arc<Mutex<RequestState>>,
}

impl BlockingHandler {
    pub fn new(state: Arc<Mutex<RequestState>>) -> Self {
        Self { state }
    }
}

impl Handler for BlockingHandler {
    fn write(&mut self, data: &[u8]) -> Result<usize, WriteError> {
        let mut state = self.state.lock().unwrap();
        state.write_data(data);
        Ok(data.len())
    }

    fn header(&mut self, data: &[u8]) -> bool {
        let mut state = self.state.lock().unwrap();
        state.push_header_data(data);
        true
    }

    fn read(&mut self, _data: &mut [u8]) -> Result<usize, curl::easy::ReadError> {
        Ok(0)
    }

    fn seek(&mut self, _whence: std::io::SeekFrom) -> curl::easy::SeekResult {
        curl::easy::SeekResult::Fail
    }
}
