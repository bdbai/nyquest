use std::io;

use super::backend::BlockingResponse;

pub struct Response {
    inner: Box<dyn BlockingResponse>,
}

impl Response {
    pub fn text(mut self) -> crate::Result<String> {
        self.inner.text()
    }

    pub fn bytes(mut self) -> crate::Result<Vec<u8>> {
        BlockingResponse::bytes(&mut *self.inner)
    }

    pub fn into_read(self) -> impl io::Read {
        self.inner
    }
}

impl From<Box<dyn BlockingResponse>> for Response {
    fn from(inner: Box<dyn BlockingResponse>) -> Self {
        Self { inner }
    }
}
