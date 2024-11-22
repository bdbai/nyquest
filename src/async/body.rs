use std::borrow::Cow;

use bytes::Bytes;
use futures_core::stream::BoxStream;

pub struct BodyStream {
    pub reader: BoxStream<'static, crate::Result<Bytes>>,
    pub size: Option<u64>,
    pub content_type: Cow<'static, str>,
}
