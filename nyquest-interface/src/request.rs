use std::borrow::Cow;

use crate::body::Body;

pub struct Request<S> {
    pub method: Cow<'static, str>,
    pub relative_uri: Cow<'static, str>,
    pub additional_headers: Vec<(Cow<'static, str>, Cow<'static, str>)>,
    pub body: Option<Body<S>>,
}
