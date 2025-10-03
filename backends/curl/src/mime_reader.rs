use crate::curl_ng::mime::MimePartReader;

pub(crate) struct DummyMimeReader;

impl MimePartReader for DummyMimeReader {
    fn read(&mut self, _data: &mut [u8]) -> Result<usize, curl::easy::ReadError> {
        unimplemented!("mime reader read")
    }
}
