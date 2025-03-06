use std::borrow::Cow;

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
