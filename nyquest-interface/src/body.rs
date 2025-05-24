//! Request body types for nyquest HTTP clients.
//!
//! This module defines the various body types that can be used in HTTP requests,
//! including byte content, form data, and multipart forms.

use std::{borrow::Cow, fmt::Debug};

#[cfg(feature = "multipart")]
#[cfg_attr(docsrs, doc(cfg(feature = "multipart")))]
mod multipart;
#[cfg(feature = "multipart")]
#[cfg_attr(docsrs, doc(cfg(feature = "multipart")))]
pub use multipart::{Part, PartBody};

/// A wrapper for streaming request body data.
pub struct SizedStream<S> {
    /// The underlying stream that provides the body data.
    pub stream: S,
    /// Optional content length of the stream, if known in advance.
    pub content_length: Option<u64>,
}

/// Represents different types of HTTP request bodies.
///
/// This enum encapsulates the various body formats that can be sent in an HTTP request,
/// including raw bytes, form data, and multipart forms.
pub enum Body<S> {
    /// Raw byte content with a specified content type.
    Bytes {
        /// The actual byte content of the body.
        content: Cow<'static, [u8]>,
        /// The MIME content type for the body.
        content_type: Cow<'static, str>,
    },
    /// URL-encoded form data.
    Form {
        /// Collection of key-value pairs representing the form fields.
        fields: Vec<(Cow<'static, str>, Cow<'static, str>)>,
    },
    /// Multipart form data, enabled with the "multipart" feature.
    #[cfg(feature = "multipart")]
    Multipart {
        /// Collection of parts that make up the multipart form.
        parts: Vec<Part<S>>,
    },
    /// Streaming body data.
    Stream {
        /// The underlying stream that provides the body data.
        stream: SizedStream<S>,
        /// The MIME content type for the stream.
        content_type: Cow<'static, str>,
    },
}

impl<S> Debug for SizedStream<S>
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
            Body::Stream {
                stream: reader,
                content_type,
            } => f
                .debug_struct("Body::Stream")
                .field("reader", reader)
                .field("content_type", content_type)
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
            Body::Stream {
                stream: reader,
                content_type,
            } => Body::Stream {
                stream: reader.clone(),
                content_type: content_type.clone(),
            },
        }
    }
}

impl<S> Clone for SizedStream<S>
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
