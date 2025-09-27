use std::{pin::Pin, ptr::null};

use pin_project_lite::pin_project;

use crate::curl_ng::{
    easy::AsRawEasyMut,
    mime::{Mime, MimePart, MimePartReader, CURLOPT_MIMEPOST},
    CurlCodeContext, WithCurlCodeContext,
};

pin_project! {
    pub struct MimeHandle<E> {
        #[pin]
        easy: E,
        mime: Option<Mime>,
    }
}

impl<E> MimeHandle<E> {
    pub fn new(easy: E) -> Self {
        MimeHandle { easy, mime: None }
    }

    pub fn as_easy_mut(self: Pin<&mut Self>) -> Pin<&mut E> {
        self.project().easy
    }
}

impl<E: AsRawEasyMut> MimeHandle<E> {
    pub fn set_mime_from_parts(
        self: Pin<&mut Self>,
        parts: impl IntoIterator<Item = MimePart<impl MimePartReader + Send + 'static>>,
    ) -> Result<(), CurlCodeContext> {
        let this = self.project();
        let mut raw = this.easy.as_raw_easy_mut();
        let mime = Mime::new(raw.as_mut(), parts)?;
        unsafe {
            raw.setopt_ptr(CURLOPT_MIMEPOST, mime.raw() as _)
                .with_easy_context("setopt CURLOPT_MIMEPOST")?
        }
        *this.mime = Some(mime);
        Ok(())
    }
    pub fn clear_mime(mut self: Pin<&mut Self>) -> Result<(), CurlCodeContext> {
        let this = self.as_mut().project();
        let raw = this.easy.as_raw_easy_mut();
        unsafe {
            raw.setopt_ptr(CURLOPT_MIMEPOST, null())
                .with_easy_context("setopt CURLOPT_MIMEPOST null")?
        }
        *this.mime = None;
        Ok(())
    }
}

impl<E: AsRawEasyMut> AsRawEasyMut for MimeHandle<E> {
    fn init(self: Pin<&mut Self>) -> Result<(), CurlCodeContext> {
        self.project().easy.init()
    }

    fn as_raw_easy_mut(self: Pin<&mut Self>) -> Pin<&mut crate::curl_ng::easy::RawEasy> {
        self.project().easy.as_raw_easy_mut()
    }

    fn reset(mut self: Pin<&mut Self>) -> Result<(), CurlCodeContext> {
        let this = self.as_mut().project();
        this.easy.reset()?;
        self.clear_mime()
    }
}
