use std::{io::ErrorKind, pin::Pin};

use curl::easy::{Easy2, Handler, List};
use nyquest_interface::{Body, Method, Request, Result as NyquestResult};

use crate::{
    curl_ng::{
        easy::{
            AsRawEasyMut, EasyCallback, EasyWithCallback, EasyWithHeaderList,
            OwnedEasyWithErrorBuf, RawEasy, Share, ShareHandle,
        },
        CurlStringList,
    },
    error::IntoNyquestResult,
    url::form_url_encode,
};

pub fn populate_request2<S, H: Handler>(
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
    for (name, value) in options.default_headers.iter().filter(|(name, _)| {
        // FIXME: This is O(len(additional) * len(default))
        !req.additional_headers
            .iter()
            .any(|(n, _)| n.eq_ignore_ascii_case(name))
    }) {
        headers
            .append(&format!("{name}: {value}"))
            .into_nyquest_result("default_headers curl_slist_append")?;
    }
    for (name, value) in &req.additional_headers {
        headers
            .append(&format!("{name}: {value}"))
            .into_nyquest_result("additional_headers curl_slist_append")?;
    }
    match req.body {
        Some(Body::Bytes {
            content,
            content_type,
        }) => {
            headers
                .append(&format!("content-type: {content_type}"))
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
                        list.append(&format!("{name}: {value}"))
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

pub type EasyHandle<C> =
    OwnedEasyWithErrorBuf<ShareHandle<EasyWithHeaderList<EasyWithCallback<RawEasy, C>>>>;
pub type BoxEasyHandle<C> = Pin<Box<EasyHandle<C>>>;
pub fn create_easy<C: EasyCallback>(callback: C, share: &Share) -> NyquestResult<BoxEasyHandle<C>> {
    let easy = RawEasy::new();
    let easy = EasyWithCallback::new(easy, callback);
    let easy = EasyWithHeaderList::new(easy);
    let easy = share.spawn_easy(easy);
    let easy = OwnedEasyWithErrorBuf::new(easy);
    let mut easy = Box::pin(easy);
    easy.as_mut().with_error_message(|e| e.init())?;
    Ok(easy)
}

pub fn populate_request<S, C: EasyCallback>(
    url: &str,
    mut req: Request<S>,
    options: &nyquest_interface::client::ClientOptions,
    easy: Pin<&mut EasyHandle<C>>,
) -> NyquestResult<()> {
    let mut headers = CurlStringList::default();
    easy.with_error_message(|mut e| {
        let mut raw = e.as_mut().as_raw_easy_mut();
        if !options.use_default_proxy {
            raw.as_mut().set_noproxy("*")?;
        }
        if let Some(user_agent) = options.user_agent.as_deref() {
            raw.as_mut().set_useragent(user_agent)?;
        }
        if options.use_cookies {
            raw.as_mut().set_cookiefile("")?;
        }
        if let Some(timeout) = options.request_timeout {
            raw.as_mut().set_timeout(timeout)?;
        }
        if options.ignore_certificate_errors {
            raw.as_mut().set_ssl_verify_peer(false)?;
        }
        if options.follow_redirects {
            raw.as_mut().set_follow_location(true)?;
        }
        raw.as_mut().set_url(url)?;
        if let Method::Other(method) = &req.method {
            if method.eq_ignore_ascii_case("head") {
                req.method = Method::Head;
            }
        }
        let need_body = req.method == Method::Post || req.method == Method::Put;
        match req.method {
            Method::Get if req.body.is_none() => raw.as_mut().set_get(true)?,
            Method::Get => raw.as_mut().set_custom_request("get")?,
            Method::Post => raw.as_mut().set_post(true)?,
            Method::Put if req.body.is_none() => raw.as_mut().set_custom_request("PUT")?,
            Method::Put => raw.as_mut().set_put(true)?,
            Method::Delete => raw.as_mut().set_custom_request("delete")?,
            Method::Patch => raw.as_mut().set_custom_request("patch")?,
            Method::Head => raw.as_mut().set_nobody(true)?,
            Method::Other(method) => raw.as_mut().set_custom_request(method)?,
        }
        for (name, value) in options.default_headers.iter().filter(|(name, _)| {
            // FIXME: This is O(len(additional) * len(default))
            !req.additional_headers
                .iter()
                .any(|(n, _)| n.eq_ignore_ascii_case(name))
        }) {
            headers.append(format!("{name}: {value}"));
        }
        for (name, value) in &req.additional_headers {
            headers.append(format!("{name}: {value}"));
        }
        match req.body {
            Some(Body::Bytes {
                content,
                content_type,
            }) => {
                headers.append(format!("content-type: {content_type}"));
                raw.as_mut().set_post_fields_copy(Some(&*content))?;
            }
            Some(Body::Stream { .. }) => {
                raw.as_mut().set_post_fields_copy(None)?;
                unimplemented!()
            }
            Some(Body::Form { fields }) => {
                let mut buf =
                    Vec::with_capacity(fields.iter().map(|(k, v)| k.len() + v.len() + 2).sum());
                for (name, value) in fields {
                    let easy = raw.as_mut().raw();
                    form_url_encode(easy, name, &mut buf);
                    buf.push(b'=');
                    form_url_encode(easy, value, &mut buf);
                    buf.push(b'&');
                }
                raw.as_mut().set_post_fields_copy(Some(&*buf))?;
            }
            #[cfg(feature = "multipart")]
            Some(Body::Multipart { parts }) => todo!(),
            None if need_body => {
                // Workaround for https://github.com/curl/curl/issues/1625
                raw.as_mut().set_post_fields_copy(Some(b""))?;
            }
            None => {}
        }
        raw.set_accept_encoding("")?;
        let header = e.as_easy_mut().as_easy_mut();
        header.set_headers(Some(headers))?;
        Ok(())
    })?;
    Ok(())
}
