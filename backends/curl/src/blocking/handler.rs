use std::sync::{Arc, Mutex};

use curl::easy::{Handler, WriteError};

use super::multi_easy::MultiEasyState;

pub struct BlockingHandler {
    state: Arc<Mutex<MultiEasyState>>,
}

impl BlockingHandler {
    pub fn new(state: Arc<Mutex<MultiEasyState>>) -> Self {
        Self { state }
    }
}

impl Handler for BlockingHandler {
    fn write(&mut self, data: &[u8]) -> Result<usize, WriteError> {
        let mut state = self.state.lock().unwrap();
        state.header_finished = true;
        // TODO: handle max response buffer size
        state.response_buffer.extend_from_slice(data);
        Ok(data.len())
    }

    fn header(&mut self, data: &[u8]) -> bool {
        let mut state = self.state.lock().unwrap();
        if data == b"\r\n" {
            let is_redirect = [301, 302, 303, 307, 308].contains(&state.temp_status_code);
            if !is_redirect {
                state.header_finished = true;
            }
        } else if data.contains(&b':') {
            state
                .response_headers_buffer
                .push(data.strip_suffix(b"\r\n").unwrap_or(data).into());
        } else {
            // More robust status code parsing
            let mut status_components = data.splitn(3, |&b| b.is_ascii_whitespace()).skip(1);
            if let Some(status) = status_components
                .next()
                .and_then(|s| std::str::from_utf8(s).ok())
                .and_then(|s| s.parse().ok())
            {
                state.temp_status_code = status;
            }
        }
        true
    }
}
