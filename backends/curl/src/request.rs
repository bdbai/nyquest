use std::ops::{Deref, DerefMut};
use std::pin::Pin;

use nyquest_interface::{Body, Method, Request, Result as NyquestResult};

use crate::curl_ng::easy::MimeHandle;
use crate::{
    curl_ng::{
        easy::{
            AsRawEasyMut, EasyCallback, EasyWithCallback, EasyWithHeaderList,
            OwnedEasyWithErrorBuf, RawEasy, Share, ShareHandle,
        },
        CurlStringList,
    },
    url::form_url_encode,
};

pub type EasyHandle<C> = OwnedEasyWithErrorBuf<
    MimeHandle<ShareHandle<EasyWithHeaderList<EasyWithCallback<RawEasy, C>>>>,
>;
pub type BoxEasyHandle<C> = Pin<Box<EasyHandle<C>>>;
pub fn create_easy<C: EasyCallback>(callback: C, share: &Share) -> NyquestResult<BoxEasyHandle<C>> {
    let easy = RawEasy::new();
    let easy = EasyWithCallback::new(easy, callback);
    let easy = EasyWithHeaderList::new(easy);
    let easy: ShareHandle<EasyWithHeaderList<EasyWithCallback<RawEasy, C>>> =
        share.spawn_easy(easy);
    let easy = MimeHandle::new(easy);
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
        raw.as_mut().set_accept_encoding("")?;
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
            Some(Body::Multipart { parts }) => {
                let mime = e.as_mut().as_easy_mut();
                mime.set_mime_from_parts(parts.into_iter().map(|p| {
                    use nyquest_interface::PartBody;

                    use crate::curl_ng::mime;

                    mime::MimePart {
                        name: p.name,
                        filename: p.filename,
                        content_type: Some(p.content_type),
                        header_list: if p.headers.is_empty() {
                            None
                        } else {
                            let mut list = CurlStringList::default();
                            for (name, value) in p.headers {
                                list.append(format!("{name}: {value}"));
                            }
                            Some(list)
                        },
                        content: match p.body {
                            PartBody::Bytes { content } => mime::MimePartContent::Data(content),
                            _ => mime::MimePartContent::Reader {
                                reader: crate::mime_reader::DummyMimeReader,
                                size: None,
                            },
                        },
                    }
                }))?;
            }
            None if need_body => {
                // Workaround for https://github.com/curl/curl/issues/1625
                raw.as_mut().set_post_fields_copy(Some(b""))?;
            }
            None => {}
        }
        let header = e.as_easy_mut().as_easy_mut().as_easy_mut();
        header.set_headers(Some(headers))?;
        Ok(())
    })?;
    Ok(())
}

pub(crate) trait AsCallbackMut {
    type C: EasyCallback;
    fn as_callback_mut(&mut self) -> &mut Self::C;
}

impl<C: EasyCallback, Ptr: Deref<Target = EasyHandle<C>> + DerefMut> AsCallbackMut for Pin<Ptr> {
    type C = C;
    fn as_callback_mut(&mut self) -> &mut Self::C {
        let this = self.as_mut();
        this.as_easy_mut()
            .as_easy_mut()
            .as_easy_mut()
            .as_easy_mut()
            .as_callback_mut()
    }
}
