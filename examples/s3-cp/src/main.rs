use std::{
    env,
    fs::File,
    io::{self, Read, Seek},
    path::Path,
    str::FromStr as _,
    time::Duration,
};

use nyquest::{
    blocking::{Body, Request},
    BlockingClient,
};
use rusty_s3::{Bucket, Credentials, S3Action as _, UrlStyle};

mod file_location;
mod s3_stream;

use file_location::FileLocation;

fn main() {
    nyquest_preset::register();

    let client = nyquest::ClientBuilder::default()
        .user_agent("curl/7.68.0 nyquest/0")
        .dangerously_ignore_certificate_errors()
        .build_blocking()
        .expect("Failed to build client");

    // setting up env
    let aws_key =
        env::var("AWS_ACCESS_KEY_ID").expect("Missing AWS_ACCESS_KEY_ID environment variable");
    let aws_secret = env::var("AWS_SECRET_ACCESS_KEY")
        .expect("Missing AWS_SECRET_ACCESS_KEY environment variable");
    let credentials = Credentials::new(aws_key, aws_secret);
    let region = env::var("AWS_REGION").expect("Missing AWS_REGION environment variable");
    let endpoint =
        env::var("AWS_ENDPOINT_URL_S3").expect("Missing AWS_ENDPOINT_URL_S3 environment variable");
    let endpoint = endpoint.parse().expect("AWS endpoint is a valid Url");

    // setting up args
    let mut args = env::args();
    args.next();
    let from_uri = args
        .next()
        .expect("Missing arg1 as S3 URI or local file path");
    let to_uri = args
        .next()
        .expect("Missing arg2 as S3 URI or local file path");
    let from_location = FileLocation::from_str(&from_uri).expect("Invalid from URI");
    let to_location = FileLocation::from_str(&to_uri).expect("Invalid to URI");

    // perform the copy
    match (from_location, to_location) {
        (FileLocation::Local { path }, FileLocation::Local { path: to_path }) => {
            std::fs::copy(&path, &to_path).expect("Failed to copy local file");
            eprintln!("Copied local file");
        }
        (FileLocation::S3 { bucket, object }, FileLocation::Local { path }) => {
            let bucket = Bucket::new(endpoint, UrlStyle::Path, bucket, region)
                .expect("Url has a valid scheme and host");
            let s3_response = s3_stream::request_file(bucket, credentials, client, object, 0);
            let mut read_stream = s3_response.stream;
            consume_to_file(&mut read_stream, &path);
        }
        (FileLocation::Local { path }, FileLocation::S3 { bucket, object }) => {
            let bucket = Bucket::new(endpoint, UrlStyle::Path, bucket, region)
                .expect("Url has a valid scheme and host");
            let content_type = mime_guess::from_path(&path).first_or_octet_stream();
            let file = File::open(path).expect("Failed to open file");
            let file_len = file.metadata().expect("Failed to get file metadata").len();
            consume_to_s3(
                &bucket,
                &credentials,
                &client,
                &object,
                file,
                file_len,
                content_type.essence_str().to_string(),
            );
        }
        (
            FileLocation::S3 { bucket, object },
            FileLocation::S3 {
                bucket: to_bucket,
                object: to_object,
            },
        ) => {
            let bucket = Bucket::new(endpoint.clone(), UrlStyle::Path, bucket, region.clone())
                .expect("Url has a valid scheme and host");
            let s3_response =
                s3_stream::request_file(bucket, credentials.clone(), client.clone(), object, 0);
            let content_length = s3_response
                .content_length
                .expect("content-length not present");
            let content_type = s3_response
                .content_type
                .unwrap_or_else(|| "application/octet-stream".into());
            let to_bucket = Bucket::new(endpoint, UrlStyle::Path, to_bucket, region)
                .expect("Url has a valid scheme and host");
            consume_to_s3(
                &to_bucket,
                &credentials,
                &client,
                &to_object,
                s3_response.stream,
                content_length,
                content_type,
            );
        }
    }
}

fn consume_to_file(from: &mut dyn io::Read, path: &Path) {
    let f = File::create(path).unwrap();
    eprintln!("Downloading file {:?}", path.display());
    let mut buff = io::BufWriter::new(f);
    io::copy(from, &mut buff).expect("Failed to copy response body to stdout");
}

fn consume_to_s3(
    bucket: &Bucket,
    credentials: &Credentials,
    client: &BlockingClient,
    object: &str,
    stream: impl Read + Seek + Send + 'static,
    content_length: u64,
    content_type: String,
) {
    let action = bucket.put_object(Some(credentials), object);
    let signed_url = action.sign(Duration::from_secs(3600));
    eprintln!("Signed URL for upload: {signed_url}");

    let response = client
        .request(Request::put(signed_url.to_string()).with_body(Body::stream(
            stream,
            content_type,
            content_length,
        )))
        .expect("Failed to get response");
    let status = response.status();
    if status != 200 {
        let text = response.text().expect("Failed to get body text");
        panic!("Bucket returned non-success response {status}: \n{text}");
    }
    eprintln!("Upload successful!");
}
