//! Blocking WinHTTP response implementation.

use nyquest_interface::blocking::BlockingResponse;
use nyquest_interface::Result as NyquestResult;

use crate::error::WinHttpResultExt;
use crate::handle::{ConnectionHandle, RequestHandle};

/// Blocking WinHTTP response.
pub struct WinHttpBlockingResponse {
    // Keep connection alive while response is being read
    #[allow(dead_code)]
    connection: ConnectionHandle,
    request: RequestHandle,
    status: u16,
    content_length: Option<u64>,
    headers: Vec<(String, String)>,
    max_response_buffer_size: u64,
}

impl WinHttpBlockingResponse {
    pub(crate) fn new(
        connection: ConnectionHandle,
        request: RequestHandle,
        status: u16,
        content_length: Option<u64>,
        headers: Vec<(String, String)>,
        max_response_buffer_size: u64,
    ) -> Self {
        Self {
            connection,
            request,
            status,
            content_length,
            headers,
            max_response_buffer_size,
        }
    }
}

#[cfg(feature = "blocking-stream")]
impl std::io::Read for WinHttpBlockingResponse {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        // Query available data
        let available = self
            .request
            .query_data_available()
            .map_err(std::io::Error::from)?;

        if available == 0 {
            return Ok(0);
        }

        // Read data
        let to_read = buf.len().min(available as usize);
        let bytes_read = self
            .request
            .read_data(&mut buf[..to_read])
            .map_err(std::io::Error::from)?;

        Ok(bytes_read as usize)
    }
}

impl BlockingResponse for WinHttpBlockingResponse {
    fn status(&self) -> u16 {
        self.status
    }

    fn content_length(&self) -> Option<u64> {
        self.content_length
    }

    fn get_header(&self, header: &str) -> NyquestResult<Vec<String>> {
        Ok(self
            .headers
            .iter()
            .filter(|(name, _)| name.eq_ignore_ascii_case(header))
            .map(|(_, value)| value.clone())
            .collect())
    }

    fn text(&mut self) -> NyquestResult<String> {
        let bytes = self.bytes()?;

        // Try to detect charset from Content-Type header
        if let Some(charset) = self.detect_charset() {
            // For now, we only handle UTF-8 and ASCII properly
            // Other charsets fall back to lossy UTF-8 conversion
            if charset.eq_ignore_ascii_case("utf-8") || charset.eq_ignore_ascii_case("us-ascii") {
                return Ok(String::from_utf8_lossy(&bytes).into_owned());
            }
        }

        Ok(String::from_utf8_lossy(&bytes).into_owned())
    }

    fn bytes(&mut self) -> NyquestResult<Vec<u8>> {
        let mut result = Vec::new();

        loop {
            // Query available data
            let available = self.request.query_data_available().into_nyquest()?;

            if available == 0 {
                break;
            }

            // Check buffer size limit
            if result.len() as u64 + available as u64 > self.max_response_buffer_size {
                return Err(nyquest_interface::Error::ResponseTooLarge);
            }

            // Read data
            let mut buffer = vec![0u8; available as usize];
            let bytes_read = self.request.read_data(&mut buffer).into_nyquest()?;

            if bytes_read == 0 {
                break;
            }

            result.extend_from_slice(&buffer[..bytes_read as usize]);
        }

        Ok(result)
    }
}

impl WinHttpBlockingResponse {
    fn detect_charset(&self) -> Option<String> {
        for (name, value) in &self.headers {
            if name.eq_ignore_ascii_case("content-type") {
                if let Some(charset_part) = value
                    .split(';')
                    .find(|s| s.trim().to_ascii_lowercase().starts_with("charset="))
                {
                    let charset = charset_part.trim().strip_prefix("charset=")?;
                    return Some(charset.trim_matches('"').to_string());
                }
            }
        }
        None
    }
}
