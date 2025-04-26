use std::borrow::Cow;
use std::io;

use nyquest_interface::{Body, Request, Result as NyquestResult};
use windows::Foundation::{IReference, PropertyValue};
use windows::Storage::Streams::IBuffer;
use windows::Web::Http::Headers::HttpMediaTypeHeaderValue;
use windows::Web::Http::{
    HttpBufferContent, HttpFormUrlEncodedContent, HttpMethod, HttpRequestMessage, IHttpContent,
};
use windows_collections::{IIterable, IKeyValuePair};
use windows_core::{Interface, HSTRING};

use crate::buffer::VecBuffer;
use crate::client::WinrtClient;
use crate::error::IntoNyquestResult;
use crate::string_pair::StringPair;
use crate::uri::build_uri;

impl WinrtClient {
    pub(crate) fn create_request<B>(&self, req: &Request<B>) -> NyquestResult<HttpRequestMessage> {
        let uri = build_uri(&self.base_url, &req.relative_uri)
            .map_err(|_| nyquest_interface::Error::InvalidUrl)?;
        let method = HttpMethod::Create(&HSTRING::from(&*req.method)).into_nyquest_result()?;
        let req_msg = HttpRequestMessage::Create(&method, &uri).into_nyquest_result()?;
        // TODO: cache method
        if !req.additional_headers.is_empty() || !self.default_content_headers.is_empty() {
            let headers = req_msg.Headers().into_nyquest_result()?;
            for (name, value) in &req.additional_headers {
                headers
                    .TryAppendWithoutValidation(&HSTRING::from(&**name), &HSTRING::from(&**value))
                    .into_nyquest_result()?;
            }
        }
        Ok(req_msg)
    }

    pub(crate) fn append_content_headers(
        &self,
        content: &IHttpContent,
        additional_headers: &[(Cow<'static, str>, Cow<'static, str>)],
    ) -> io::Result<()> {
        let headers = content.Headers()?;
        for (name, value) in additional_headers {
            if is_header_name_content_related(name) {
                headers.TryAppendWithoutValidation(
                    &HSTRING::from(&**name),
                    &HSTRING::from(&**value),
                )?;
            }
        }
        for (name, value) in &self.default_content_headers {
            headers.TryAppendWithoutValidation(name, value)?;
        }
        Ok(())
    }
}

pub(crate) fn is_header_name_content_related(name: &str) -> bool {
    name.get(..8)
        .filter(|n| n.eq_ignore_ascii_case("content-"))
        .is_some()
        || ["expires", "last-modified"]
            .iter()
            .any(|h| h.eq_ignore_ascii_case(name))
}

fn create_content_from_bytes(
    content: Cow<'static, [u8]>,
    content_type: Cow<'static, str>,
) -> io::Result<IHttpContent> {
    let content_len = content.len();
    let content =
        HttpBufferContent::CreateFromBuffer(&IBuffer::from(VecBuffer::new(content.into_owned())))?;
    let content_type = HttpMediaTypeHeaderValue::Create(&HSTRING::from(&*content_type))?;
    let headers = content.Headers()?;
    headers.SetContentType(&content_type)?;
    let len = PropertyValue::CreateUInt64(content_len as u64)?;
    headers.SetContentLength(&len.cast::<IReference<u64>>()?)?;
    Ok(content.cast()?)
}

pub(crate) fn create_body<S>(
    body: Body<S>,
    map_stream: &mut impl FnMut(S) -> io::Result<IHttpContent>,
) -> io::Result<IHttpContent> {
    Ok(match body {
        Body::Bytes {
            content,
            content_type,
        } => create_content_from_bytes(content, content_type)?,
        Body::Form { fields } => {
            let pairs: Vec<_> = fields
                .into_iter()
                .map(|(k, v)| {
                    Some(IKeyValuePair::from(StringPair(
                        HSTRING::from(&*k),
                        HSTRING::from(&*v),
                    )))
                })
                .collect();
            let content = HttpFormUrlEncodedContent::Create(&IIterable::from(pairs))?;
            content.cast()?
        }
        #[cfg(feature = "multipart")]
        Body::Multipart { parts } => {
            use nyquest_interface::PartBody;

            let content = windows::Web::Http::HttpMultipartFormDataContent::new()?;
            for part in parts {
                let part_content = match part.body {
                    PartBody::Bytes { content } => {
                        create_content_from_bytes(content, part.content_type)?
                    }
                    PartBody::Stream(stream) => map_stream(stream.stream)?,
                };
                let headers = part_content.Headers()?;
                for (name, value) in part.headers {
                    headers.TryAppendWithoutValidation(
                        &HSTRING::from(&*name),
                        &HSTRING::from(&*value),
                    )?;
                }
                match part.filename {
                    Some(filename) => content.AddWithNameAndFileName(
                        &part_content.cast::<IHttpContent>()?,
                        &HSTRING::from(&*part.name),
                        &HSTRING::from(&*filename),
                    )?,
                    None => content.AddWithName(
                        &part_content.cast::<IHttpContent>()?,
                        &HSTRING::from(&*part.name),
                    )?,
                };
            }
            content.cast()?
        }
        Body::Stream(stream) => map_stream(stream.stream)?,
    })
}
