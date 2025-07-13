use std::io::ErrorKind;

use curl::easy::{Easy2, Handler, List};
use nyquest_interface::{Body, Method, Request};

use crate::error::IntoNyquestResult;

pub fn populate_request<S, H: Handler>(
    url: &str,
    req: Request<S>,
    options: &nyquest_interface::client::ClientOptions,
    easy: &mut Easy2<H>,
    _populate_stream: impl FnOnce(&mut Easy2<H>, S) -> nyquest_interface::Result<()>,
) -> nyquest_interface::Result<()> {
    if !options.use_default_proxy {
        easy.noproxy("*")
            .into_nyquest_result("set CURLOPT_NOPROXY")?;
    }
    if let Some(user_agent) = options.user_agent.as_deref() {
        easy.useragent(user_agent)
            .into_nyquest_result("set CURLOPT_USERAGENT")?;
    }
    if options.use_cookies {
        easy.cookie_file("")
            .into_nyquest_result("set CURLOPT_COOKIEFILE")?;
    }
    if let Some(timeout) = options.request_timeout {
        easy.timeout(timeout)
            .into_nyquest_result("set CURLOPT_TIMEOUT")?;
    }
    if options.ignore_certificate_errors {
        easy.ssl_verify_peer(false)
            .into_nyquest_result("set CURLOPT_SSL_VERIFYPEER")?;
    }
    if options.follow_redirects {
        easy.follow_location(true)
            .into_nyquest_result("set CURLOPT_FOLLOWLOCATION")?;
    }
    easy.url(url).into_nyquest_result("set CURLOPT_URL")?;
    match &req.method {
        Method::Get if req.body.is_none() => easy.get(true),
        Method::Get => easy.custom_request("get"),
        Method::Post => easy.post(true),
        Method::Put if req.body.is_none() => easy.custom_request("PUT"),
        Method::Put => easy.put(true),
        Method::Delete => easy.custom_request("delete"),
        Method::Patch => easy.custom_request("patch"),
        Method::Head => easy.nobody(true),
        Method::Other(method) => easy.custom_request(method),
    }
    .into_nyquest_result("set CURLOPT_CUSTOMREQUEST")?;
    let mut headers = List::new();
    for (name, value) in &options.default_headers {
        headers
            .append(&format!("{}: {}", name, value))
            .into_nyquest_result("default_headers curl_slist_append")?;
    }
    for (name, value) in &req.additional_headers {
        headers
            .append(&format!("{}: {}", name, value))
            .into_nyquest_result("additional_headers curl_slist_append")?;
    }
    match req.body {
        Some(Body::Bytes {
            content,
            content_type,
        }) => {
            headers
                .append(&format!("content-type: {}", content_type))
                .into_nyquest_result("set content-type curl_slist_append")?;
            easy.post_fields_copy(&content)
                .into_nyquest_result("set CURLOPT_COPYPOSTFIELDS")?;
        }
        Some(Body::Stream { .. }) => unimplemented!(),
        Some(Body::Form { fields }) => {
            let mut buf =
                String::with_capacity(fields.iter().map(|(k, v)| k.len() + v.len() + 2).sum());
            for (name, value) in fields {
                buf.push_str(&easy.url_encode(name.as_bytes()));
                buf.push('=');
                buf.push_str(&easy.url_encode(value.as_bytes()));
                buf.push('&');
            }
            easy.post_fields_copy(buf.replace("%20", "+").as_bytes())
                .into_nyquest_result("set CURLOPT_COPYPOSTFIELDS")?;
        }
        #[cfg(feature = "multipart")]
        Some(Body::Multipart { parts }) => {
            use std::io;

            use nyquest_interface::PartBody;

            let mut form = curl::easy::Form::new();
            for part in parts {
                let mut formpart = form.part(&part.name);
                if !part.headers.is_empty() {
                    let mut list = List::new();
                    for (name, value) in &part.headers {
                        list.append(&format!("{}: {}", name, value))
                            .into_nyquest_result("multipart header curl_slist_append")?;
                    }
                    formpart.content_header(list);
                }
                match &part.body {
                    PartBody::Bytes { content } => {
                        formpart.buffer(
                            part.filename.as_deref().unwrap_or_default(),
                            content.to_vec(),
                        );
                        formpart.content_type(&part.content_type);
                    }
                    PartBody::Stream(_) => {
                        if let Some(filename) = &part.filename {
                            formpart.filename(&**filename);
                        }
                        return Err(nyquest_interface::Error::Io(io::Error::new(
                            ErrorKind::InvalidInput,
                            "unsupported body type",
                        )));
                    }
                }
                formpart
                    .add()
                    .map_err(|e| nyquest_interface::Error::Io(io::Error::other(e.to_string())))?;
            }
            easy.httppost(form)
                .into_nyquest_result("set CURLOPT_HTTPPOST")?;
        }
        None if req.method == Method::Post || req.method == Method::Put => {
            // Workaround for https://github.com/curl/curl/issues/1625
            easy.post_fields_copy(b"")
                .into_nyquest_result("set require_body CURLOPT_COPYPOSTFIELDS")?;
        }
        None => {}
    }
    easy.http_headers(headers)
        .into_nyquest_result("set CURLOPT_HTTPHEADER")?;
    easy.accept_encoding("")
        .into_nyquest_result("set CURLOPT_ACCEPT_ENCODING")?;
    Ok(())
}
