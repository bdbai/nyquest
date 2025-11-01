use std::ptr::NonNull;

use objc2::{
    rc::{autoreleasepool, Retained},
    AnyThread,
};
use objc2_core_foundation::{kCFStringEncodingInvalidId, CFString};
use objc2_foundation::{ns_string, NSString, NSStringEncoding, NSUTF8StringEncoding};

use crate::datatask::DataTaskSharedContextRetained;

pub(crate) struct NSUrlSessionResponse {
    pub(crate) response: Retained<objc2_foundation::NSHTTPURLResponse>,
    pub(crate) task: Retained<objc2_foundation::NSURLSessionDataTask>,
    pub(crate) shared: DataTaskSharedContextRetained,
}

impl NSUrlSessionResponse {
    pub(crate) fn status(&self) -> u16 {
        unsafe { self.response.statusCode() as u16 }
    }

    pub(crate) fn content_length(&self) -> Option<u64> {
        match unsafe { self.response.expectedContentLength() } {
            -1 => None,
            len => Some(len as u64),
        }
    }

    pub(crate) fn get_header(&self, header: &str) -> nyquest_interface::Result<Vec<String>> {
        let value = unsafe {
            self.response
                .valueForHTTPHeaderField(&NSString::from_str(header))
        };
        Ok(value
            .map(|v| autoreleasepool(|pool| unsafe { v.to_str(pool).to_owned() }))
            .into_iter()
            .collect())
    }

    fn detect_response_encoding(&self) -> Option<NSStringEncoding> {
        let content_type = unsafe {
            self.response
                .valueForHTTPHeaderField(ns_string!("Content-Type"))?
        };
        let cf_encoding: u32 = autoreleasepool(|pool| {
            let content_type = unsafe { content_type.to_str(pool) };
            let (_, mut charset) = content_type
                .split(';')
                .filter_map(|s| s.split_once('='))
                .find(|(k, _)| k.trim().eq_ignore_ascii_case("charset"))?;
            charset = charset.trim_matches('"');
            let cf_encoding =
                CFString::convert_iana_char_set_name_to_encoding(&CFString::from_str(charset));
            Some(cf_encoding).filter(|&e| e != kCFStringEncodingInvalidId)
        })?;
        let ns_encoding = CFString::convert_encoding_to_ns_string_encoding(cf_encoding);
        Some(ns_encoding as _)
    }
    pub(crate) fn convert_bytes_to_string(
        &self,
        bytes: Vec<u8>,
    ) -> nyquest_interface::Result<String> {
        let mut encoding = self
            .detect_response_encoding()
            .unwrap_or(NSUTF8StringEncoding);

        loop {
            if encoding == NSUTF8StringEncoding {
                return Ok(String::from_utf8(bytes)
                    .unwrap_or_else(|e| String::from_utf8_lossy(e.as_bytes()).into_owned()));
            }
            unsafe {
                let nsstr = NSString::initWithBytesNoCopy_length_encoding_freeWhenDone(
                    NSString::alloc(),
                    NonNull::new_unchecked(bytes.as_ptr() as _),
                    bytes.len() as _,
                    encoding,
                    false,
                );
                let Some(nsstr) = nsstr else {
                    encoding = NSUTF8StringEncoding;
                    continue;
                };
                let str = autoreleasepool(|pool| nsstr.to_str(pool).to_owned());
                return Ok(str);
            };
        }
    }

    pub(crate) fn consume_response_to_buffer(
        &mut self,
        buf: &mut [u8],
    ) -> Option<std::io::Result<usize>> {
        use nyquest_interface::Error as NyquestError;
        use objc2_foundation::NSURLSessionTaskState;

        let read_len = self.shared.with_response_buffer_for_stream_mut(|data| {
            let read_len = if data.len() > buf.len() {
                unsafe {
                    // Triggering a suspend when the task is already suspended can cause it to not
                    // wake up.
                    if self.task.state() == NSURLSessionTaskState::Running {
                        self.task.suspend();
                    }
                }
                buf.len()
            } else {
                data.len()
            };
            buf[..read_len].copy_from_slice(&data[..read_len]);
            data.drain(..read_len);
            read_len
        });
        Some(match read_len {
            Ok(0) if self.shared.is_completed() => Ok(0),
            Ok(0) => return None,
            Ok(read_len) => Ok(read_len),
            Err(NyquestError::Io(e)) => Err(e),
            Err(NyquestError::RequestTimeout) => Err(std::io::ErrorKind::TimedOut.into()),
            Err(e) => unreachable!("Unexpected error: {e}"),
        })
    }
}

impl Drop for NSUrlSessionResponse {
    fn drop(&mut self) {
        unsafe {
            self.task.cancel();
        }
    }
}
