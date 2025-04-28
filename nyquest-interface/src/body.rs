use std::{borrow::Cow, fmt::Debug};

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

#[cfg(feature = "multipart")]
pub struct Part<S> {
    pub headers: Vec<(Cow<'static, str>, Cow<'static, str>)>,
    pub name: Cow<'static, str>,
    pub filename: Option<Cow<'static, str>>,
    pub content_type: Cow<'static, str>,
    pub body: PartBody<S>,
}

#[cfg(feature = "multipart")]
pub enum PartBody<S> {
    Bytes { content: Cow<'static, [u8]> },
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

impl<S> Debug for Part<S>
where
    S: Debug,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Part")
            .field("headers", &self.headers)
            .field("name", &self.name)
            .field("filename", &self.filename)
            .field("content_type", &self.content_type)
            .field("body", &self.body)
            .finish()
    }
}

impl<S> Debug for PartBody<S>
where
    S: Debug,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PartBody::Bytes { content } => f
                .debug_struct("PartBody::Bytes")
                .field("content", content)
                .finish(),
            PartBody::Stream(stream) => f
                .debug_struct("PartBody::Stream")
                .field("stream", stream)
                .finish(),
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
impl<S> Clone for Part<S>
where
    S: Clone,
{
    fn clone(&self) -> Self {
        Self {
            headers: self.headers.clone(),
            name: self.name.clone(),
            filename: self.filename.clone(),
            content_type: self.content_type.clone(),
            body: self.body.clone(),
        }
    }
}
impl<S> Clone for PartBody<S>
where
    S: Clone,
{
    fn clone(&self) -> Self {
        match self {
            PartBody::Bytes { content } => PartBody::Bytes {
                content: content.clone(),
            },
            PartBody::Stream(stream) => PartBody::Stream(stream.clone()),
        }
    }
}
