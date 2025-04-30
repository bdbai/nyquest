use std::{borrow::Cow, fmt::Debug};

use super::StreamReader;

pub struct Part<S> {
    pub headers: Vec<(Cow<'static, str>, Cow<'static, str>)>,
    pub name: Cow<'static, str>,
    pub filename: Option<Cow<'static, str>>,
    pub content_type: Cow<'static, str>,
    pub body: PartBody<S>,
}

pub enum PartBody<S> {
    Bytes { content: Cow<'static, [u8]> },
    Stream(StreamReader<S>),
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
