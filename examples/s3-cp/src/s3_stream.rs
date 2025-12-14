use std::{
    io::{self, Read, Seek, SeekFrom},
    time::Duration,
};

use nyquest::{
    blocking::{ReadStream, Response},
    BlockingClient, Request,
};
use rusty_s3::{Bucket, Credentials, S3Action as _};

pub struct S3Stream {
    bucket: Bucket,
    credentials: Credentials,
    client: BlockingClient,
    object: String,

    position: u64,
    stream: ReadStream,
}

pub struct S3Response {
    pub stream: S3Stream,
    pub content_length: Option<u64>,
    pub content_type: Option<String>,
}

impl Read for S3Stream {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let bytes_read = self.stream.read(buf)?;
        self.position += bytes_read as u64;
        Ok(bytes_read)
    }
}

impl Seek for S3Stream {
    fn seek(&mut self, pos: SeekFrom) -> io::Result<u64> {
        let new_position = match pos {
            SeekFrom::Start(offset) => offset,
            SeekFrom::End(_) => {
                unimplemented!("Seeking from end is not supported for S3 streams")
            }
            SeekFrom::Current(offset) => {
                if offset >= 0 {
                    self.position.checked_add(offset as u64).ok_or_else(|| {
                        io::Error::new(io::ErrorKind::InvalidInput, "Seek position overflow")
                    })?
                } else {
                    self.position.checked_sub((-offset) as u64).ok_or_else(|| {
                        io::Error::new(io::ErrorKind::InvalidInput, "Seek position underflow")
                    })?
                }
            }
        };

        if new_position != self.position {
            let new_stream = request(
                &self.bucket,
                &self.credentials,
                &self.client,
                &self.object,
                new_position,
            )
            .into_read();
            self.stream = new_stream;
            self.position = new_position;
        }

        Ok(self.position)
    }
}

pub fn request_file(
    bucket: Bucket,
    credentials: Credentials,
    client: BlockingClient,
    object: String,
    offset: u64,
) -> S3Response {
    let response = request(&bucket, &credentials, &client, &object, offset);
    let content_length = response
        .get_header("content-length")
        .unwrap_or_default()
        .into_iter()
        .next()
        .and_then(|v| v.parse().ok());
    let content_type = response
        .get_header("content-type")
        .unwrap_or_default()
        .into_iter()
        .next();
    eprintln!("S3 Response Content-Length: {content_length:?}, Content-Type: {content_type:?}",);
    let stream = S3Stream {
        bucket,
        credentials,
        client,
        object,
        position: offset,
        stream: response.into_read(),
    };
    S3Response {
        stream,
        content_length,
        content_type,
    }
}

fn request(
    bucket: &Bucket,
    credentials: &Credentials,
    client: &BlockingClient,
    object: &str,
    offset: u64,
) -> Response {
    let presigned_url_duration = Duration::from_secs(60);
    let mut action = bucket.get_object(Some(credentials), object);
    let range = format!("bytes={}-", offset);
    action.headers_mut().append("range", range.clone());
    let signed_url = action.sign(presigned_url_duration);
    eprintln!("Signed URL: {signed_url}");

    let response = client
        .request(Request::get(signed_url.to_string()).with_header("range", range))
        .expect("Failed to get response");
    let status = response.status();
    if !status.is_successful() {
        let text = response.text().expect("Failed to get body text");
        panic!("Bucket returned non-success response {status}: \n{text}");
    }
    response
}
