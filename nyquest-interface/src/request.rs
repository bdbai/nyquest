//! HTTP request definitions for nyquest HTTP clients.
//!
//! This module provides the core request types used to construct and send
//! HTTP requests through nyquest backends.

use std::{borrow::Cow, fmt::Debug};

use crate::body::Body;

/// HTTP request methods supported by nyquest.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Method {
    /// HTTP GET method
    Get,
    /// HTTP POST method
    Post,
    /// HTTP PUT method
    Put,
    /// HTTP DELETE method
    Delete,
    /// HTTP PATCH method
    Patch,
    /// HTTP HEAD method
    Head,
    /// Other HTTP methods not explicitly enumerated
    Other(Cow<'static, str>),
}

/// Represents an HTTP request to be sent by a nyquest client.
pub struct Request<S> {
    /// The HTTP method for this request
    pub method: Method,
    /// The URI for this request, can be absolute or relative
    pub relative_uri: Cow<'static, str>,
    /// Additional HTTP headers to include with this request
    pub additional_headers: Vec<(Cow<'static, str>, Cow<'static, str>)>,
    /// Optional request body
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
