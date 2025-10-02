use std::borrow::Cow;

use http::{header::CONTENT_TYPE, HeaderMap, HeaderName, HeaderValue};
use nyquest_interface::Method;
use reqwest::{Client, RequestBuilder};
use url::Url;

use crate::{
    client::ReqwestClient,
    error::{ReqwestBackendError, Result},
};

pub fn convert_method(method: Method) -> Result<reqwest::Method> {
    Ok(match method {
        Method::Get => reqwest::Method::GET,
        Method::Post => reqwest::Method::POST,
        Method::Put => reqwest::Method::PUT,
        Method::Delete => reqwest::Method::DELETE,
        Method::Head => reqwest::Method::HEAD,
        Method::Patch => reqwest::Method::PATCH,
        Method::Other(other) => other
            .parse()
            .map_err(|_| ReqwestBackendError::InvalidMethod)?,
    })
}

pub fn build_url(base_url: Option<&Url>, url: &str) -> Result<Url> {
    match base_url {
        Some(base) => base.join(url),
        None => Url::parse(url),
    }
    .map_err(|_| ReqwestBackendError::InvalidUrl(url.to_string()))
}

fn convert_header_name(s: Cow<'static, str>) -> Result<HeaderName> {
    match &s {
        Cow::Borrowed(s) if !s.bytes().any(|c| c.is_ascii_uppercase()) => {
            return Ok(HeaderName::from_static(s))
        }
        Cow::Borrowed(s) => HeaderName::from_bytes(s.as_bytes()),
        Cow::Owned(s) => HeaderName::from_bytes(s.as_bytes()),
    }
    .map_err(|_| ReqwestBackendError::InvalidHeaderName(s.into_owned()))
}

fn convert_header_value(k: &str, v: Cow<'static, str>) -> Result<HeaderValue> {
    match v {
        Cow::Borrowed(s) => Ok(HeaderValue::from_static(s)),
        Cow::Owned(s) => HeaderValue::from_bytes(s.as_bytes())
            .map_err(|_| ReqwestBackendError::InvalidHeaderValue(k.into())),
    }
}

fn build_request_generic<S>(
    client: &Client,
    base_url: Option<&Url>,
    req: nyquest_interface::Request<S>,
    mut transform_body: impl FnMut(S) -> (reqwest::Body, Option<usize>),
) -> nyquest_interface::Result<RequestBuilder> {
    let url = build_url(base_url, &req.relative_uri).map_err(nyquest_interface::Error::from)?;
    let method = convert_method(req.method)?;

    let mut request_builder = client.request(method, url);

    // Add request headers
    for (key, value) in req.additional_headers {
        let value = convert_header_value(&key, value)?;
        request_builder = request_builder.header(convert_header_name(key)?, value);
    }

    // Add body
    match req.body {
        None => {}
        Some(nyquest_interface::Body::Bytes {
            content: Cow::Borrowed(content),
            content_type,
        }) => {
            request_builder = request_builder
                .header(CONTENT_TYPE, &*content_type)
                .body(content);
        }
        Some(nyquest_interface::Body::Bytes {
            content: Cow::Owned(content),
            content_type,
        }) => {
            request_builder = request_builder
                .header(CONTENT_TYPE, &*content_type)
                .body(content);
        }
        Some(nyquest_interface::Body::Form { fields }) => {
            request_builder = request_builder.form(&fields);
        }
        Some(nyquest_interface::Body::Stream {
            content_type,
            stream,
        }) => {
            request_builder = request_builder
                .header("content-type", &*content_type)
                .body(transform_body(stream).0);
        }
        #[cfg(feature = "multipart")]
        Some(nyquest_interface::Body::Multipart { parts }) => {
            let mut form = reqwest::multipart::Form::new();
            for part in parts {
                use std::iter;

                let headers = part
                    .headers
                    .into_iter()
                    .map(|(k, v)| {
                        let value = convert_header_value(&k, v)?;
                        Ok((convert_header_name(k)?, value))
                    })
                    .chain(iter::once(Ok((
                        CONTENT_TYPE,
                        convert_header_value("content-type", part.content_type)?,
                    ))))
                    .collect::<Result<HeaderMap>>()?;

                match part.body {
                    nyquest_interface::PartBody::Bytes { content } => {
                        let mut part_builder = reqwest::multipart::Part::bytes(content);
                        if let Some(filename) = part.filename {
                            part_builder = part_builder.file_name(filename);
                        }
                        form = form.part(part.name, part_builder.headers(headers));
                    }
                    nyquest_interface::PartBody::Stream(stream) => {
                        let mut part_builder =
                            reqwest::multipart::Part::stream(transform_body(stream).0);
                        if let Some(filename) = part.filename {
                            part_builder = part_builder.file_name(filename);
                        }
                        form = form.part(part.name, part_builder.headers(headers));
                    }
                }
            }
            request_builder = request_builder.multipart(form);
        }
    }

    Ok(request_builder)
}

impl ReqwestClient {
    pub fn request<S>(
        &self,
        req: nyquest_interface::Request<S>,
        transform_body: impl FnMut(S) -> (reqwest::Body, Option<usize>),
    ) -> nyquest_interface::Result<reqwest::RequestBuilder> {
        #[allow(unused_mut)]
        let mut builder =
            build_request_generic(&self.client, self.base_url.as_ref(), req, transform_body)?;
        #[cfg(target_arch = "wasm32")]
        if let Some(timeout) = self.timeout {
            builder = builder.timeout(timeout);
        }
        Ok(builder)
    }
}
