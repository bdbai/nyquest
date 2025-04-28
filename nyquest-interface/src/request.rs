use std::{borrow::Cow, fmt::Debug};

use crate::body::Body;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Method {
    Get,
    Post,
    Put,
    Delete,
    Patch,
    Other(Cow<'static, str>),
}

pub struct Request<S> {
    pub method: Method,
    pub relative_uri: Cow<'static, str>,
    pub additional_headers: Vec<(Cow<'static, str>, Cow<'static, str>)>,
    pub body: Option<Body<S>>,
}

impl<S> Debug for Request<S>
where
    Body<S>: Debug,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Request")
            .field("method", &self.method)
            .field("relative_uri", &self.relative_uri)
            .field("additional_headers", &self.additional_headers)
            .field("body", &self.body)
            .finish()
    }
}

impl<S> Clone for Request<S>
where
    Body<S>: Clone,
{
    fn clone(&self) -> Self {
        Self {
            method: self.method.clone(),
            relative_uri: self.relative_uri.clone(),
            additional_headers: self.additional_headers.clone(),
            body: self.body.clone(),
        }
    }
}
