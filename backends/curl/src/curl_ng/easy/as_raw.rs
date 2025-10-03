use std::pin::Pin;

use crate::curl_ng::{easy::RawEasy, CurlCodeContext};

pub trait AsRawEasyMut {
    fn init(self: Pin<&mut Self>) -> Result<(), CurlCodeContext>;
    fn as_raw_easy_mut(self: Pin<&mut Self>) -> Pin<&mut RawEasy>;
    fn reset(self: Pin<&mut Self>) -> Result<(), CurlCodeContext>;
}
