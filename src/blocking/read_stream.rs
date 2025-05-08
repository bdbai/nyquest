use std::io;

use nyquest_interface::blocking::AnyBlockingResponse;

/// An [`std::io::Read`] stream backed by a blocking response.
pub struct ReadStream {
    inner: Box<dyn AnyBlockingResponse>,
}

impl ReadStream {
    pub(crate) fn new(inner: Box<dyn AnyBlockingResponse>) -> Self {
        Self { inner }
    }
}

impl io::Read for ReadStream {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.inner.read(buf)
    }
}

mod trait_assert {
    trait _AssertMarker: Send + Sync {}
    impl _AssertMarker for super::ReadStream {}
}
