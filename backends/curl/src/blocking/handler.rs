use std::sync::{Arc, Mutex};

use curl::easy::{Handler, WriteError};
use nyquest_interface::{blocking::BoxedStream, SizedStream};

use crate::state::RequestState;

pub struct BlockingHandler {
    state: Arc<Mutex<RequestState>>,
    body_stream: Option<SizedStream<BoxedStream>>,
}

impl BlockingHandler {
    pub fn new(state: Arc<Mutex<RequestState>>) -> Self {
        Self {
            state,
            body_stream: None,
        }
    }

    pub fn set_body_stream(&mut self, body_stream: SizedStream<BoxedStream>) {
        self.body_stream = Some(body_stream);
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

    fn read(&mut self, data: &mut [u8]) -> Result<usize, curl::easy::ReadError> {
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
        match stream.stream.seek(whence) {
            Ok(_pos) => curl::easy::SeekResult::Ok,
            // FIXME: propagate IO errors
            Err(_e) => curl::easy::SeekResult::Fail,
        }
    }
}
