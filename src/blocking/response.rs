use std::io;

use nyquest_interface::blocking::AnyBlockingResponse;

pub struct Response {
    inner: Box<dyn AnyBlockingResponse>,
}

impl Response {
    pub fn status(&self) -> u16 {
        self.inner.status()
    }

    pub fn content_length(&self) -> Option<u64> {
        self.inner.content_length()
    }

    pub fn get_header(&self, header: &str) -> crate::Result<Vec<String>> {
        Ok(self.inner.get_header(header)?)
    }

    pub fn text(mut self) -> crate::Result<String> {
        Ok(self.inner.text()?)
    }

    pub fn bytes(mut self) -> crate::Result<Vec<u8>> {
        Ok(AnyBlockingResponse::bytes(&mut *self.inner)?)
    }

    pub fn into_read(self) -> impl io::Read {
        self.inner
    }
}

impl From<Box<dyn AnyBlockingResponse>> for Response {
    fn from(inner: Box<dyn AnyBlockingResponse>) -> Self {
        Self { inner }
    }
}
