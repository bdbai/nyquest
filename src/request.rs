use std::borrow::Cow;

use nyquest_interface::Request as RequestImpl;

use crate::body::Body;

pub struct Request<S> {
    pub(crate) inner: RequestImpl<S>,
}

impl<S> Request<S> {
    pub fn new(
        method: impl Into<Cow<'static, str>>,
        relative_uri: impl Into<Cow<'static, str>>,
    ) -> Self {
        Self {
            inner: RequestImpl {
                method: method.into(),
                relative_uri: relative_uri.into(),
                additional_headers: vec![],
                body: None,
            },
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

    pub fn patch(uri: impl Into<Cow<'static, str>>) -> Self {
        Self::new("PATCH", uri)
    }

    pub fn with_header(
        mut self,
        name: impl Into<Cow<'static, str>>,
        value: impl Into<Cow<'static, str>>,
    ) -> Self {
        self.inner
            .additional_headers
            .push((name.into(), value.into()));
        self
    }

    pub fn with_body(mut self, body: Body<S>) -> Self {
        self.inner.body = Some(body.inner);
        self
    }
}
