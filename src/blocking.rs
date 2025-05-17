//! Blocking client support.
//!
//! The blocking client will block the current thread to execute.
//!

use std::borrow::Cow;

use nyquest_interface::blocking::BoxedStream;

pub(crate) mod client;
mod read_stream;
mod response;

/// The Request Body type for blocking requests.
pub type Body = crate::body::Body<BoxedStream>;
/// The Request type for blocking requests.
pub type Request = crate::Request<BoxedStream>;
/// The multipart form part type for blocking requests.
#[cfg(feature = "multipart")]
pub type Part = crate::body::Part<BoxedStream>;
/// The multipart form part body type for blocking requests.
#[cfg(feature = "multipart")]
pub type PartBody = crate::body::PartBody<BoxedStream>;
pub use read_stream::ReadStream;
pub use response::Response;

/// Shortcut method to quickly make a `GET` request.
///
/// See also the methods on the [`Response`] type.
///
/// **Note**: This function creates a new internal [`BlockingClient`] on each call, and so should
/// not be used if making many requests. Create a [`BlockingClient`] instead.
///
/// [`BlockingClient`]: crate::BlockingClient
pub fn get(uri: impl Into<Cow<'static, str>>) -> crate::Result<Response> {
    let client = crate::client::ClientBuilder::default().build_blocking()?;
    client.request(Request::get(uri))
}
