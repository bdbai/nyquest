use std::io::ErrorKind;

use curl::easy::{Easy, List};
use nyquest_interface::{Body, Request};

use crate::{error::IntoNyquestResult, urlencoded::curl_escape};

pub fn populate_request<S>(
    url: &str,
    req: &Request<S>,
    options: &nyquest_interface::client::ClientOptions,
    easy: &mut Easy,
) -> nyquest_interface::Result<()> {
    if !options.use_default_proxy {
        easy.noproxy("*")
            .into_nyquest_result("set CURLOPT_NOPROXY")?;
    }
    if let Some(user_agent) = options.user_agent.as_deref() {
        easy.useragent(user_agent).expect("set curl user agent");
    }
    easy.url(url).into_nyquest_result("set CURLOPT_URL")?;
    match &*req.method {
        "GET" | "get" if req.body.is_none() => easy.get(true),
        "POST" | "post" => easy.post(true),
        "PUT" | "put" => easy.put(true),
        method => easy.custom_request(method),
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
    match &req.body {
        Some(Body::Bytes {
            content,
            content_type,
        }) => {
            headers
                .append(&format!("content-type: {}", content_type))
                .into_nyquest_result("set content-type curl_slist_append")?;
            easy.post_fields_copy(content)
                .into_nyquest_result("set CURLOPT_COPYPOSTFIELDS")?;
        }
        Some(Body::Stream(_)) => unimplemented!(),
        Some(Body::Form { fields }) => {
            let mut buf =
                Vec::with_capacity(fields.iter().map(|(k, v)| k.len() + v.len() + 2).sum());
            for (name, value) in fields {
                buf.extend_from_slice(&curl_escape(easy, &**name));
                buf.push(b'=');
                buf.extend_from_slice(&curl_escape(easy, &**value));
                buf.push(b'&');
            }
            buf.pop();
            easy.post_fields_copy(&buf)
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
                formpart.add().map_err(|e| {
                    nyquest_interface::Error::Io(io::Error::new(ErrorKind::Other, e.to_string()))
                })?;
            }
            easy.httppost(form)
                .into_nyquest_result("set CURLOPT_HTTPPOST")?;
        }
        None => {}
    }
    easy.http_headers(headers)
        .into_nyquest_result("set CURLOPT_HTTPHEADER")?;
    easy.accept_encoding("")
        .into_nyquest_result("set CURLOPT_ACCEPT_ENCODING")?;
    Ok(())
}
