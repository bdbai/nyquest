use std::io;

use windows::{core::HSTRING, Foundation::Uri};

pub(crate) fn build_uri(base: &Option<HSTRING>, relative: &str) -> io::Result<Uri> {
    let uri = if let Some(base_url) = &base {
        Uri::CreateWithRelativeUri(base_url, &HSTRING::from(relative))?
    } else {
        Uri::CreateUri(&HSTRING::from(relative))?
    };
    Ok(uri)
}
