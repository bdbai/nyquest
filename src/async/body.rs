use std::borrow::Cow;

use bytes::Bytes;
use futures_core::stream::BoxStream;

pub enum Body {
    Bytes {
        content: Cow<'static, [u8]>,
        content_type: Cow<'static, str>,
    },
    Form {
        fields: Vec<(Cow<'static, str>, Cow<'static, str>)>,
    },
    Multipart {
        parts: Vec<Part>,
    },
    Stream {
        reader: BoxStream<'static, crate::Result<Bytes>>,
        size: Option<u64>,
        content_type: Cow<'static, str>,
    },
}

pub struct Part {
    pub headers: Vec<(Cow<'static, str>, Cow<'static, str>)>,
    pub filename: Option<Cow<'static, str>>,
    pub body: Body,
}

impl Body {
    pub fn text(
        text: impl Into<Cow<'static, str>>,
        content_type: impl Into<Cow<'static, str>>,
    ) -> Self {
        Self::Bytes {
            content: match text.into() {
                Cow::Borrowed(s) => Cow::Borrowed(s.as_bytes()),
                Cow::Owned(s) => Cow::Owned(s.into_bytes()),
            },
            content_type: content_type.into(),
        }
    }

    pub fn bytes(
        bytes: impl Into<Cow<'static, [u8]>>,
        content_type: impl Into<Cow<'static, str>>,
    ) -> Self {
        Self::Bytes {
            content: bytes.into(),
            content_type: content_type.into(),
        }
    }
}
