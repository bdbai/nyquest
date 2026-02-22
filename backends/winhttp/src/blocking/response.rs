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
    max_response_buffer_size: Option<u64>,
}

impl WinHttpBlockingResponse {
    pub(crate) fn new(
        connection: ConnectionHandle,
        request: RequestHandle,
        status: u16,
        content_length: Option<u64>,
        max_response_buffer_size: Option<u64>,
    ) -> Self {
        Self {
            connection,
            request,
            status,
            content_length,
            max_response_buffer_size,
        }
    }
}

#[cfg(feature = "blocking-stream")]
impl std::io::Read for WinHttpBlockingResponse {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        // Query available data
        let available = self.request.query_data_available()?;

        if available == 0 {
            return Ok(0);
        }

        // Read data
        let to_read = buf.len().min(available as usize);
        let buf = &mut buf[..to_read];
        {
            let uninit =
                unsafe { std::slice::from_raw_parts_mut(buf.as_mut_ptr() as *mut _, buf.len()) };
            let bytes_read = self.request.read_data(uninit)?;
            Ok(bytes_read as usize)
        }
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
        let headers = self.request.query_header(header)?;
        Ok(headers)
    }

    fn text(&mut self) -> NyquestResult<String> {
        let bytes = self.bytes()?;

        #[cfg(feature = "charset")]
        if let Some((_, mut charset)) = self
            .get_header("content-type")?
            .pop()
            .unwrap_or_default()
            .split(';')
            .filter_map(|s| s.split_once('='))
            .find(|(k, _)| k.trim().eq_ignore_ascii_case("charset"))
        {
            charset = charset.trim_matches('"');
            if let Ok(decoded) = iconv_native::decode_lossy(&bytes, charset.trim()) {
                return Ok(decoded);
            }
        }

        Ok(String::from_utf8_lossy(&bytes).into_owned())
    }

    fn bytes(&mut self) -> NyquestResult<Vec<u8>> {
        let mut result = Vec::new();
        let mut max_to_read = match self.max_response_buffer_size {
            Some(max_size) => max_size as usize + 1,
            None => usize::MAX,
        };

        loop {
            let available = self.request.query_data_available().into_nyquest()?;

            if available == 0 {
                break;
            }

            let offset = result.len();
            let to_read = (available as usize).min(max_to_read);
            result.reserve(to_read);

            let capacity = result.spare_capacity_mut();
            let bytes_read = self.request.read_data(capacity).into_nyquest()?;
            if bytes_read == 0 {
                break;
            }
            unsafe { result.set_len(offset + bytes_read as usize) };

            if bytes_read as usize >= max_to_read {
                return Err(nyquest_interface::Error::ResponseTooLarge);
            }
            max_to_read -= bytes_read as usize;
        }

        Ok(result)
    }
}
