//! Multipart form data definitions for HTTP requests.
//!
//! This module defines types for creating multipart/form-data bodies,
//! which allow sending complex data including files in HTTP requests.

use std::{borrow::Cow, fmt::Debug};

use super::StreamReader;

/// Represents a part in a multipart form.
///
/// Each part has a name, optional filename, content type, and a body,
/// along with optional additional headers.
pub struct Part<S> {
    /// Additional headers for this part.
    pub headers: Vec<(Cow<'static, str>, Cow<'static, str>)>,
    /// Name of the form field.
    pub name: Cow<'static, str>,
    /// Optional filename for file parts.
    pub filename: Option<Cow<'static, str>>,
    /// MIME content type for this part.
    pub content_type: Cow<'static, str>,
    /// Body content for this part.
    pub body: PartBody<S>,
}

/// Body content for a multipart form part.
///
/// This can be either raw bytes or a stream.
pub enum PartBody<S> {
    /// Raw byte content.
    Bytes {
        /// The bytes that make up this part's content.
        content: Cow<'static, [u8]>,
    },
    /// Streaming content for larger part bodies.
    #[doc(hidden)]
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
