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
    pub fn plain_text(text: impl Into<Cow<'static, str>>) -> Self {
        Self::text(text, "text/plain")
    }
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

    pub fn binary_bytes(bytes: impl Into<Cow<'static, [u8]>>) -> Self {
        Self::bytes(bytes, "application/octet-stream")
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
    #[cfg(feature = "json")]
    pub fn json<T: serde::Serialize>(value: &T) -> serde_json::Result<Self> {
        let bytes = serde_json::to_vec(value)?;
        Ok(Self::json_bytes(bytes))
    }

    pub fn form(fields: impl IntoIterator<Item = (Cow<'static, str>, Cow<'static, str>)>) -> Self {
        Self {
            inner: BodyImpl::Form {
                fields: fields.into_iter().collect(),
            },
        }
    }

    #[cfg(feature = "multipart")]
    pub fn multipart(parts: impl IntoIterator<Item = Part<S>>) -> Self {
        Self {
            inner: BodyImpl::Multipart {
                parts: parts.into_iter().map(|part| part.inner).collect(),
            },
        }
    }
}

/// Constructs a form body from a predefined set of fields.
/// # Examples
/// ```
/// use std::borrow::Cow;
/// use nyquest::{blocking::Body, body_form};
///
/// let body: Body = body_form! {
///     "key1" => "value1",
///     "key2" => String::from("value2"),
///     Cow::Borrowed("key3") => "value3",
/// };
/// ```
#[macro_export]
macro_rules! body_form {
    ($($key:expr => $value:expr),* $(,)?) => {
        ::nyquest::__private::Body::form(vec![
            $(
                (
                    ::std::convert::Into::<::std::borrow::Cow::<'static, str>>::into($key),
                    ::std::convert::Into::<::std::borrow::Cow::<'static, str>>::into($value)
                ),
            )*
        ])
    };
}

#[cfg(feature = "multipart")]
impl<S> Part<S> {
    pub fn new_with_content_type(
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

    /// # Note
    /// 
    /// Support for per-part headers is subject to underlying implementation. For example,
    /// `winrt` backend only supports well-known content headers as listed [here](https://learn.microsoft.com/en-us/uwp/api/windows.web.http.headers.httpcontentheadercollection?view=winrt-26100#properties).
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
