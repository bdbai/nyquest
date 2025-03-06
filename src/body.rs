use std::borrow::Cow;

use nyquest_interface::Body as BodyImpl;
#[cfg(feature = "multipart")]
use nyquest_interface::{Part as PartImpl, PartBody as PartBodyImpl, StreamReader};

pub struct Body<S> {
    pub(crate) inner: BodyImpl<S>,
}

#[cfg(feature = "multipart")]
pub struct Part<S> {
    inner: PartImpl<S>,
}

#[cfg(feature = "multipart")]
pub struct PartBody<S> {
    inner: PartBodyImpl<S>,
}

impl<S> Body<S> {
    pub fn text(
        text: impl Into<Cow<'static, str>>,
        content_type: impl Into<Cow<'static, str>>,
    ) -> Self {
        Self {
            inner: BodyImpl::Bytes {
                content: match text.into() {
                    Cow::Borrowed(s) => Cow::Borrowed(s.as_bytes()),
                    Cow::Owned(s) => Cow::Owned(s.into_bytes()),
                },
                content_type: content_type.into(),
            },
        }
    }

    pub fn bytes(
        bytes: impl Into<Cow<'static, [u8]>>,
        content_type: impl Into<Cow<'static, str>>,
    ) -> Self {
        Self {
            inner: BodyImpl::Bytes {
                content: bytes.into(),
                content_type: content_type.into(),
            },
        }
    }

    pub fn json_bytes(bytes: impl Into<Cow<'static, [u8]>>) -> Self {
        Self::bytes(bytes, "application/json")
    }
}

#[cfg(feature = "multipart")]
impl<S> Part<S> {
    pub fn new(
        name: impl Into<Cow<'static, str>>,
        content_type: impl Into<Cow<'static, str>>,
        body: PartBody<S>,
    ) -> Self {
        Self {
            inner: PartImpl {
                headers: vec![],
                name: name.into(),
                filename: None,
                content_type: content_type.into(),
                body: body.inner,
            },
        }
    }

    pub fn with_header(
        mut self,
        name: impl Into<Cow<'static, str>>,
        value: impl Into<Cow<'static, str>>,
    ) -> Self {
        self.inner.headers.push((name.into(), value.into()));
        self
    }

    pub fn with_filename(mut self, filename: impl Into<Cow<'static, str>>) -> Self {
        self.inner.filename = Some(filename.into());
        self
    }
}

#[cfg(feature = "multipart")]
impl<S> PartBody<S> {
    pub fn text(text: impl Into<Cow<'static, str>>) -> Self {
        Self {
            inner: PartBodyImpl::Bytes {
                content: match text.into() {
                    Cow::Borrowed(s) => Cow::Borrowed(s.as_bytes()),
                    Cow::Owned(s) => Cow::Owned(s.into_bytes()),
                },
            },
        }
    }

    pub fn bytes(bytes: impl Into<Cow<'static, [u8]>>) -> Self {
        Self {
            inner: PartBodyImpl::Bytes {
                content: bytes.into(),
            },
        }
    }

    pub fn stream(stream: S, content_length: Option<u64>) -> Self {
        Self {
            inner: PartBodyImpl::Stream(StreamReader {
                stream,
                content_length,
            }),
        }
    }
}
