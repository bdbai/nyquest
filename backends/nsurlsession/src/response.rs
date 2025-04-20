use objc2::rc::{autoreleasepool, Retained};
use objc2_foundation::NSString;

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
}
