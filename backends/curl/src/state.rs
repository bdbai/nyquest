#[derive(Debug, Default)]
pub(crate) struct RequestState {
    pub(crate) temp_status_code: u16,
    pub(crate) is_established: bool,
    pub(crate) header_finished: bool,
    pub(crate) response_headers_buffer: Vec<Vec<u8>>,
    pub(crate) response_buffer: Vec<u8>,
}

impl RequestState {
    pub(crate) fn push_header_data(&mut self, data: &[u8]) -> bool {
        if data == b"\r\n" {
            let is_redirect = [301, 302, 303, 307, 308].contains(&self.temp_status_code);
            // TODO: handle direct
            if !is_redirect && !self.is_established {
                self.header_finished = true;
                return true;
            }
        } else if data.contains(&b':') {
            self.response_headers_buffer
                .push(data.strip_suffix(b"\r\n").unwrap_or(data).into());
        } else {
            let mut status_components = data.splitn(3, u8::is_ascii_whitespace).skip(1);

            if let Some(status) = status_components
                .next()
                .and_then(|s| std::str::from_utf8(s).ok())
                .and_then(|s| s.parse().ok())
            {
                self.temp_status_code = status;
            }
            self.is_established = status_components
                .next()
                .map(|s| s.eq_ignore_ascii_case(b"connection established\r\n"))
                .unwrap_or(false);
        }
        false
    }

    pub(crate) fn write_data(&mut self, data: &[u8]) {
        self.header_finished = true;
        // TODO: handle max response buffer size
        self.response_buffer.extend_from_slice(data);
    }
}
