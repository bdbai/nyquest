use std::{borrow::Cow, fmt::Debug};

use nyquest_interface::{Method as MethodImpl, Request as RequestImpl};

use crate::body::Body;

/// The Request Method (VERB)
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Method {
    inner: MethodImpl,
}

/// A request generic over async or blocking stream.
pub struct Request<S> {
    pub(crate) inner: RequestImpl<S>,
}

impl Method {
    /// Constructs a method from a string.
    pub fn custom(method: impl Into<Cow<'static, str>>) -> Self {
        Self {
            inner: MethodImpl::Other(method.into()),
        }
    }

    /// Constructs a `GET` method.
    pub fn get() -> Self {
        Self {
            inner: MethodImpl::Get,
        }
    }

    /// Constructs a `POST` method.
    pub fn post() -> Self {
        Self {
            inner: MethodImpl::Post,
        }
    }

    /// Constructs a `PUT` method.
    pub fn put() -> Self {
        Self {
            inner: MethodImpl::Put,
        }
    }

    /// Constructs a `DELETE` method.
    pub fn delete() -> Self {
        Self {
            inner: MethodImpl::Delete,
        }
    }

    /// Constructs a `PATCH` method.
    pub fn patch() -> Self {
        Self {
            inner: MethodImpl::Patch,
        }
    }
}

impl<S> Request<S> {
    /// Constructs a request with the given method and a relative or absolute URI.
    ///
    /// If the URI is relative, it will be resolved against the [`crate::ClientBuilder::base_url`]
    /// option that was used to create the client, if specified.
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

    /// Constructs a request with the given method and a relative or absolute URI.
    ///
    /// See [`Request::new`] for more details.
    pub fn get(uri: impl Into<Cow<'static, str>>) -> Self {
        Self::new(Method::get(), uri)
    }

    /// Constructs a request with the given method and a relative or absolute URI.
    ///
    /// See [`Request::new`] for more details.
    pub fn post(uri: impl Into<Cow<'static, str>>) -> Self {
        Self::new(Method::post(), uri)
    }

    /// Constructs a request with the given method and a relative or absolute URI.
    ///
    /// See [`Request::new`] for more details.
    pub fn put(uri: impl Into<Cow<'static, str>>) -> Self {
        Self::new(Method::put(), uri)
    }

    /// Constructs a request with the given method and a relative or absolute URI.
    ///
    /// See [`Request::new`] for more details.
    pub fn delete(uri: impl Into<Cow<'static, str>>) -> Self {
        Self::new(Method::delete(), uri)
    }

    /// Constructs a request with the given method and a relative or absolute URI.
    ///
    /// See [`Request::new`] for more details.
    pub fn patch(uri: impl Into<Cow<'static, str>>) -> Self {
        Self::new(Method::patch(), uri)
    }

    /// Attach a request header to the request.
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

    /// Set the request body of the request.
    ///
    /// When called multiple times, the last call will override any previous body.
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
