use std::io;
use std::pin::Pin;
use std::task::Context;
use std::task::Poll;

use nyquest_interface::r#async::futures_io;
use nyquest_interface::r#async::AnyAsyncResponse;

/// A [`futures_io::AsyncRead`] stream backed by an async response.
pub struct AsyncReadStream {
    inner: Pin<Box<dyn AnyAsyncResponse>>,
}

impl AsyncReadStream {
    pub(crate) fn new(inner: Pin<Box<dyn AnyAsyncResponse>>) -> Self {
        Self { inner }
    }
}

impl futures_io::AsyncRead for AsyncReadStream {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut [u8],
    ) -> Poll<io::Result<usize>> {
        Pin::new(&mut self.inner).poll_read(cx, buf)
    }
}

mod trait_assert {
    trait _AssertMarker: Send + Sync + Unpin {}
    impl _AssertMarker for super::AsyncReadStream {}
}
