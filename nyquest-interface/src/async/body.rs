use futures_io::{AsyncRead, AsyncSeek};

pub trait BodyStream: AsyncRead + AsyncSeek + Send {}

pub type BoxedStream = Box<dyn BodyStream>;
pub type Body = crate::body::Body<BoxedStream>;

impl Body {
    #[doc(hidden)]
    pub fn stream<S: AsyncRead + AsyncSeek + Send + 'static>(
        stream: S,
        content_length: Option<u64>,
    ) -> Self {
        crate::body::Body::Stream(crate::body::StreamReader {
            stream: Box::new(stream),
            content_length,
        })
    }
}

impl<S: AsyncRead + AsyncSeek + Send + ?Sized> BodyStream for S {}
