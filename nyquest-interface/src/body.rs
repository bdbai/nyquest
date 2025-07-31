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
        stream: S,
        /// The MIME content type for the stream.
        content_type: Cow<'static, str>,
    },
}

impl<S> Debug for Body<S>
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
            Body::Multipart { parts: _ } => f
                .debug_struct("Body::Multipart")
                .finish(),
            Body::Stream {
                stream: _,
                content_type,
            } => f
                .debug_struct("Body::Stream")
                .field("content_type", content_type)
                .finish(),
        }
    }
}
