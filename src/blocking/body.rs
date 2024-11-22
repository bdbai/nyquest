use std::borrow::Cow;

pub struct BodyStream {
    pub reader: Box<dyn std::io::Read + Send>,
    pub size: Option<u64>,
    pub content_type: Cow<'static, str>,
}
