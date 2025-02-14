use std::io::{Read, Seek};

pub trait BodyStream: Read + Seek + Send {}

pub(super) type BoxedStream = Box<dyn BodyStream>;
pub type Body = crate::body::Body<BoxedStream>;

impl Body {
    #[doc(hidden)]
    pub fn stream<S: Read + Seek + Send + 'static>(stream: S, content_length: Option<u64>) -> Self {
        crate::body::Body::Stream(crate::body::StreamReader {
            stream: Box::new(stream),
            content_length,
        })
    }
}

impl<S: Read + Seek + Send + ?Sized> BodyStream for S {}
