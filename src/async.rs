use std::borrow::Cow;

use nyquest_interface::r#async::BoxedStream;

pub(crate) mod client;
mod response;

pub type Body = crate::body::Body<BoxedStream>;
pub type Request = crate::Request<BoxedStream>;
#[cfg(feature = "multipart")]
pub type Part = crate::body::Part<BoxedStream>;
#[cfg(feature = "multipart")]
pub type PartBody = crate::body::PartBody<BoxedStream>;
pub use response::Response;

pub async fn get(uri: Cow<'static, str>) -> crate::Result<Response> {
    let client = crate::client::ClientBuilder::default()
        .build_async()
        .await
        .map_err(|e| match e {
            crate::client::BuildClientError::NoBackend => Err(e).unwrap(),
            crate::client::BuildClientError::BackendError(e) => e,
        })?;
    client.request(Request::get(uri)).await
}
