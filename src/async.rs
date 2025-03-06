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
