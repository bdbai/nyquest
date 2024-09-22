use super::any::AnyAsyncResponse;

pub struct Response {
    inner: Box<dyn AnyAsyncResponse>,
}

impl Response {
    pub fn status(&self) -> u16 {
        self.inner.status()
    }

    pub fn content_length(&self) -> Option<u64> {
        self.inner.content_length()
    }

    pub fn get_header(&self, header: &str) -> crate::Result<Vec<String>> {
        self.inner.get_header(header)
    }

    pub async fn text(mut self) -> crate::Result<String> {
        self.inner.text().await
    }

    pub async fn bytes(mut self) -> crate::Result<Vec<u8>> {
        self.inner.bytes().await
    }

    // TODO: stream
}

impl From<Box<dyn AnyAsyncResponse>> for Response {
    fn from(inner: Box<dyn AnyAsyncResponse>) -> Self {
        Self { inner }
    }
}
