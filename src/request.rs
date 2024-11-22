use std::borrow::Cow;

use crate::body::Body;

pub struct Request<S> {
    pub method: Cow<'static, str>,
    pub relative_uri: Cow<'static, str>,
    pub additional_headers: Vec<(Cow<'static, str>, Cow<'static, str>)>,
    pub body: Option<Body<S>>,
}

impl<S> Request<S> {
    pub fn new(
        method: impl Into<Cow<'static, str>>,
        relative_uri: impl Into<Cow<'static, str>>,
    ) -> Self {
        Self {
            method: method.into(),
            relative_uri: relative_uri.into(),
            additional_headers: vec![],
            body: None,
        }
    }

    pub fn get(uri: impl Into<Cow<'static, str>>) -> Self {
        Self::new("GET", uri)
    }

    pub fn post(uri: impl Into<Cow<'static, str>>) -> Self {
        Self::new("POST", uri)
    }

    pub fn put(uri: impl Into<Cow<'static, str>>) -> Self {
        Self::new("PUT", uri)
    }

    pub fn delete(uri: impl Into<Cow<'static, str>>) -> Self {
        Self::new("DELETE", uri)
    }

    pub fn with_header(
        mut self,
        name: impl Into<Cow<'static, str>>,
        value: impl Into<Cow<'static, str>>,
    ) -> Self {
        self.additional_headers.push((name.into(), value.into()));
        self
    }

    pub fn with_body(mut self, body: Body<S>) -> Self {
        self.body = Some(body);
        self
    }
}
