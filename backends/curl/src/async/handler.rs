use std::sync::Arc;

use curl::easy::{Handler, WriteError};

use super::r#loop::{EasyPause, SharedRequestContext};

pub struct AsyncHandler {
    ctx: Arc<SharedRequestContext>,
    pause: EasyPause,
}

impl AsyncHandler {
    pub fn new(ctx: Arc<SharedRequestContext>, pause: EasyPause) -> Self {
        Self { ctx, pause }
    }
}

impl Handler for AsyncHandler {
    fn write(&mut self, data: &[u8]) -> Result<usize, WriteError> {
        let mut state = self.ctx.state.lock().unwrap();
        state.header_finished = true;
        // TODO: handle max response buffer size
        state.response_buffer.extend_from_slice(data);
        drop(state);
        unsafe {
            self.pause.pause();
        }
        self.ctx.waker.wake();
        Ok(data.len())
    }

    fn header(&mut self, data: &[u8]) -> bool {
        let mut state = self.ctx.state.lock().unwrap();
        if data == b"\r\n" {
            let is_redirect = [301, 302, 303, 307, 308].contains(&state.temp_status_code);
            // TODO: handle direct
            if !is_redirect && !state.is_established {
                state.header_finished = true;
                unsafe {
                    self.pause.pause();
                }
            }
        } else if data.contains(&b':') {
            state
                .response_headers_buffer
                .push(data.strip_suffix(b"\r\n").unwrap_or(data).into());
        } else {
            let mut status_components = data.splitn(3, u8::is_ascii_whitespace).skip(1);

            if let Some(status) = status_components
                .next()
                .and_then(|s| std::str::from_utf8(s).ok())
                .and_then(|s| s.parse().ok())
            {
                state.temp_status_code = status;
            }
            state.is_established = status_components
                .next()
                .map(|s| s.eq_ignore_ascii_case(b"connection established\r\n"))
                .unwrap_or(false);
        }
        drop(state);
        self.ctx.waker.wake();
        true
    }
}
