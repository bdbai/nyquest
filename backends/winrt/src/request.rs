use std::borrow::Cow;
use std::io;

use nyquest_interface::{Body, Request};
use windows::Foundation::{IReference, PropertyValue, Uri};
use windows::Storage::Streams::IBuffer;
use windows::Web::Http::Headers::HttpMediaTypeHeaderValue;
use windows::Web::Http::{
    HttpBufferContent, HttpFormUrlEncodedContent, HttpMethod, HttpRequestMessage, IHttpContent,
};
use windows_collections::{IIterable, IKeyValuePair};
use windows_core::{Interface, HSTRING};

use crate::buffer::VecBuffer;
use crate::string_pair::StringPair;

pub(crate) fn create_request<B>(uri: &Uri, req: &Request<B>) -> io::Result<HttpRequestMessage> {
    let method = HttpMethod::Create(&HSTRING::from(&*req.method))?;
    let req_msg = HttpRequestMessage::Create(&method, uri)?;
    // TODO: cache method
    if !req.additional_headers.is_empty() {
        let headers = req_msg.Headers()?;
        for (name, value) in &req.additional_headers {
            headers.Append(&HSTRING::from(&**name), &HSTRING::from(&**value))?;
        }
    }
    req_msg.SetRequestUri(uri)?;
    Ok(req_msg)
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
                    headers.Append(&HSTRING::from(&*name), &HSTRING::from(&*value))?;
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
