use std::future::Future;

use nyquest_interface::Error as NyquestError;

use crate::curl_ng::{
    mime::{DummyMimePartReader, MimePartContent},
    CurlCodeContext,
};

pub(super) struct ReadTaskCollection {}

impl ReadTaskCollection {
    pub fn new(_: impl Send) -> Self {
        Self {}
    }

    pub fn add_in_handler(&mut self, _: impl Send, _: impl Send) -> Result<(), CurlCodeContext> {
        Ok(())
    }
    pub fn add_mime_part_reader(&mut self, _: impl Send) -> MimePartContent<DummyMimePartReader> {
        unreachable!("async-stream feature is disabled")
    }

    pub fn execute(&self, _: &impl Send) -> impl Future<Output = Result<(), NyquestError>> + Send {
        std::future::pending()
    }
}
