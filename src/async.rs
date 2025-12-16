//! `async` client support.

use std::borrow::Cow;

#[cfg(feature = "async-stream")]
use nyquest_interface::r#async::{BoxedStream, SizedBodyStream, UnsizedBodyStream};

#[cfg(feature = "async-stream")]
mod async_read_stream;
pub(crate) mod client;
mod response;

#[cfg(not(feature = "async-stream"))]
type BoxedStream = std::convert::Infallible;

/// The Request Body type for async requests.
pub type Body = crate::body::Body<BoxedStream>;
/// The Request type for async requests.
pub type Request = crate::Request<BoxedStream>;
/// The multipart form part type for async requests.
#[cfg(feature = "multipart")]
pub type Part = crate::body::Part<BoxedStream>;
/// The multipart form part body type for async requests.
#[cfg(feature = "multipart")]
pub type PartBody = crate::body::PartBody<BoxedStream>;
#[cfg(feature = "async-stream")]
pub use async_read_stream::AsyncReadStream;
pub use response::Response;

#[cfg(feature = "async-stream")]
use crate::body::private::{IntoSizedStream, IntoUnsizedStream};

/// Shortcut method to quickly make a `GET` request.
///
/// See also the methods on the [`Response`] type.
///
/// **Note**: This function creates a new internal [`AsyncClient`] on each call, and so should not
/// be used if making many requests. Create a [`AsyncClient`] instead.
///
/// [`AsyncClient`]: crate::AsyncClient
pub async fn get(uri: impl Into<Cow<'static, str>>) -> crate::Result<Response> {
    let client = crate::client::ClientBuilder::default()
        .build_async()
        .await?;
    client.request(Request::get(uri)).await
}

#[cfg(feature = "async-stream")]
impl<S: SizedBodyStream> IntoSizedStream<BoxedStream> for S {
    fn into_stream(self, size: u64) -> BoxedStream {
        BoxedStream::Sized {
            stream: Box::pin(self),
            content_length: size,
        }
    }
}

#[cfg(feature = "async-stream")]
impl<S: UnsizedBodyStream> IntoUnsizedStream<BoxedStream> for S {
    fn into_stream(self) -> BoxedStream {
        BoxedStream::Unsized {
            stream: Box::pin(self),
        }
    }
}
