use std::{borrow::Cow, fmt::Debug};

use nyquest_interface::{Method as MethodImpl, Request as RequestImpl};

use crate::body::Body;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Method {
    inner: MethodImpl,
}

pub struct Request<S> {
    pub(crate) inner: RequestImpl<S>,
}

impl Method {
    pub fn custom(method: impl Into<Cow<'static, str>>) -> Self {
        Self {
            inner: MethodImpl::Other(method.into()),
        }
    }

    pub fn get() -> Self {
        Self {
            inner: MethodImpl::Get,
        }
    }

    pub fn post() -> Self {
        Self {
            inner: MethodImpl::Post,
        }
    }

    pub fn put() -> Self {
        Self {
            inner: MethodImpl::Put,
        }
    }

    pub fn delete() -> Self {
        Self {
            inner: MethodImpl::Delete,
        }
    }

    pub fn patch() -> Self {
        Self {
            inner: MethodImpl::Patch,
        }
    }
}

impl<S> Request<S> {
    pub fn new(method: Method, relative_uri: impl Into<Cow<'static, str>>) -> Self {
        Self {
            inner: RequestImpl {
                method: method.inner,
                relative_uri: relative_uri.into(),
                additional_headers: vec![],
                body: None,
            },
        }
    }

    pub fn get(uri: impl Into<Cow<'static, str>>) -> Self {
        Self::new(Method::get(), uri)
    }

    pub fn post(uri: impl Into<Cow<'static, str>>) -> Self {
        Self::new(Method::post(), uri)
    }

    pub fn put(uri: impl Into<Cow<'static, str>>) -> Self {
        Self::new(Method::put(), uri)
    }

    pub fn delete(uri: impl Into<Cow<'static, str>>) -> Self {
        Self::new(Method::delete(), uri)
    }

    pub fn patch(uri: impl Into<Cow<'static, str>>) -> Self {
        Self::new(Method::patch(), uri)
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

impl<S> Debug for Request<S>
where
    RequestImpl<S>: Debug,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.inner.fmt(f)
    }
}

impl<S> Clone for Request<S>
where
    RequestImpl<S>: Clone,
{
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
        }
    }
}
