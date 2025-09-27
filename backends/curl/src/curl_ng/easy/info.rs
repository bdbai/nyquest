use crate::curl_ng::{easy::RawEasy, CurlCodeContext, WithCurlCodeContext};

impl RawEasy {
    pub(super) unsafe fn getinfo_long(
        &self,
        info: curl_sys::CURLINFO,
        context: &'static str,
    ) -> Result<libc::c_long, CurlCodeContext> {
        unsafe {
            let mut val: libc::c_long = 0;
            curl_sys::curl_easy_getinfo(self.raw(), info, &mut val).with_easy_context(context)?;
            Ok(val)
        }
    }

    pub(super) unsafe fn getinfo_off_t(
        &self,
        info: curl_sys::CURLINFO,
        context: &'static str,
    ) -> Result<curl_sys::curl_off_t, CurlCodeContext> {
        unsafe {
            let mut val: curl_sys::curl_off_t = 0;
            curl_sys::curl_easy_getinfo(self.raw(), info, &mut val).with_easy_context(context)?;
            Ok(val)
        }
    }

    pub fn get_response_code(&self) -> Result<u16, CurlCodeContext> {
        let code = unsafe {
            self.getinfo_long(
                curl_sys::CURLINFO_RESPONSE_CODE,
                "getinfo CURLINFO_RESPONSE_CODE",
            )?
        };
        Ok(code as u16)
    }

    pub fn get_content_length(&self) -> Result<Option<u64>, CurlCodeContext> {
        const CURLINFO_CONTENT_LENGTH_DOWNLOAD_T: i32 = 0x600000 + 15;

        let len = unsafe {
            self.getinfo_off_t(
                CURLINFO_CONTENT_LENGTH_DOWNLOAD_T,
                "getinfo CURLINFO_CONTENT_LENGTH_DOWNLOAD_T",
            )?
        };
        if len < 0 {
            Ok(None)
        } else {
            Ok(Some(len as u64))
        }
    }
}
