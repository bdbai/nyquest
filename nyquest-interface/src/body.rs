use std::{borrow::Cow, fmt::Debug};

#[cfg(feature = "multipart")]
#[cfg_attr(docsrs, doc(cfg(feature = "multipart")))]
mod multipart;
#[cfg(feature = "multipart")]
#[cfg_attr(docsrs, doc(cfg(feature = "multipart")))]
pub use multipart::{Part, PartBody};

pub struct StreamReader<S> {
    pub stream: S,
    pub content_length: Option<u64>,
}

pub enum Body<S> {
    Bytes {
        content: Cow<'static, [u8]>,
        content_type: Cow<'static, str>,
    },
    Form {
        fields: Vec<(Cow<'static, str>, Cow<'static, str>)>,
    },
    #[cfg(feature = "multipart")]
    Multipart {
        parts: Vec<Part<S>>,
    },
    Stream(StreamReader<S>),
}

impl<S> Debug for StreamReader<S>
where
    S: Debug,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StreamReader")
            .field("stream", &self.stream)
            .field("content_length", &self.content_length)
            .finish()
    }
}

impl<S> Debug for Body<S>
where
    S: Debug,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Body::Bytes {
                content,
                content_type,
            } => f
                .debug_struct("Body::Bytes")
                .field("content", content)
                .field("content_type", content_type)
                .finish(),
            Body::Form { fields } => f
                .debug_struct("Body::Form")
                .field("fields", fields)
                .finish(),
            #[cfg(feature = "multipart")]
            Body::Multipart { parts } => f
                .debug_struct("Body::Multipart")
                .field("parts", parts)
                .finish(),
            Body::Stream(stream) => f
                .debug_struct("Body::Stream")
                .field("stream", stream)
                .finish(),
        }
    }
}

impl<S> Clone for Body<S>
where
    S: Clone,
{
    fn clone(&self) -> Self {
        match self {
            Body::Bytes {
                content,
                content_type,
            } => Body::Bytes {
                content: content.clone(),
                content_type: content_type.clone(),
            },
            Body::Form { fields } => Body::Form {
                fields: fields.clone(),
            },
            #[cfg(feature = "multipart")]
            Body::Multipart { parts } => Body::Multipart {
                parts: parts.clone(),
            },
            Body::Stream(stream) => Body::Stream(stream.clone()),
        }
    }
}

impl<S> Clone for StreamReader<S>
where
    S: Clone,
{
    fn clone(&self) -> Self {
        Self {
            stream: self.stream.clone(),
            content_length: self.content_length,
        }
    }
}
