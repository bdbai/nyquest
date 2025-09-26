//! MIME handling in libcurl.

mod ffi;
mod part;
mod raw;

use std::{ffi::c_void, pin::Pin, ptr::NonNull};

use crate::curl_ng::{
    easy::RawEasy,
    ffi::{list_to_raw, transform_cow_str_to_c_str},
    CurlCodeContext, WithCurlCodeContext,
};
use raw::RawMime;

pub(super) use ffi::CURLOPT_MIMEPOST;
use libc::{c_char, c_int, size_t};
pub use part::{MimePart, MimePartContent, MimePartReader};

#[derive(Debug)]
pub struct Mime {
    raw: RawMime,
}

impl Mime {
    pub(crate) fn new<R: MimePartReader + Send + 'static>(
        easy: Pin<&mut RawEasy>,
        parts: impl IntoIterator<Item = MimePart<R>>,
    ) -> Result<Self, CurlCodeContext> {
        let raw = unsafe { ffi::curl_mime_init(easy.raw()) };
        let raw = NonNull::new(raw).expect("curl_mime_init failed alloc mime");
        let raw = RawMime(raw);

        for mut part in parts {
            let part_raw = unsafe { ffi::curl_mime_addpart(raw.0.as_ptr()) };
            assert!(!part_raw.is_null());

            unsafe {
                ffi::curl_mime_name(part_raw, transform_cow_str_to_c_str(&mut part.name))
                    .with_easy_context("curl_mime_name")?;

                if let Some(filename) = &mut part.filename {
                    ffi::curl_mime_filename(part_raw, transform_cow_str_to_c_str(filename))
                        .with_easy_context("curl_mime_filename")?;
                }
                if let Some(content_type) = &mut part.content_type {
                    ffi::curl_mime_type(part_raw, transform_cow_str_to_c_str(content_type))
                        .with_easy_context("curl_mime_type")?;
                }
                if let Some(header_list) = part.header_list {
                    let list_raw = list_to_raw(Some(&header_list));
                    ffi::curl_mime_headers(part_raw, list_raw, 1)
                        .with_easy_context("curl_mime_headers")?;
                    std::mem::forget(header_list);
                }

                match part.content {
                    MimePartContent::Data(ref mut data) => {
                        ffi::curl_mime_data(
                            part_raw,
                            data.as_ref().as_ptr() as *const c_char,
                            data.len() as size_t,
                        )
                        .with_easy_context("curl_mime_data")?;
                    }
                    MimePartContent::Reader { reader, size } => {
                        let reader = Box::new(reader);
                        let reader_ptr = Box::into_raw(reader);
                        ffi::curl_mime_data_cb(
                            part_raw,
                            size.unwrap_or(-1) as curl_sys::curl_off_t,
                            Some(part::read_cb::<R>),
                            Some(part::seek_cb::<R>),
                            Some(part::free_cb::<R>),
                            reader_ptr as *mut c_void,
                        )
                        .with_easy_context("curl_mime_data_cb")?;
                    }
                }
            }
        }

        Ok(Self { raw })
    }

    pub fn raw(&self) -> *mut ffi::curl_mime {
        self.raw.0.as_ptr()
    }
}
