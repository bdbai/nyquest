use std::{fmt::Debug, io};

use nyquest_interface::blocking::AnyBlockingResponse;

use crate::StatusCode;

/// A blocking HTTP response.
pub struct Response {
    inner: Box<dyn AnyBlockingResponse>,
}

impl Response {
    /// Get the `StatusCode` of this Response.
    pub fn status(&self) -> StatusCode {
        self.inner.status().into()
    }

    /// Return the response as-is, or [`crate::Error::NonSuccessfulStatusCode`] if the status code
    /// does not indicate success.
    #[inline]
    pub fn with_successful_status(self) -> crate::Result<Self> {
        let status = self.status();
        if status.is_successful() {
            Ok(self)
        } else {
            Err(crate::Error::NonSuccessfulStatusCode(status))
        }
    }

    /// Get the `content-length` of this response, if known by the backend.
    pub fn content_length(&self) -> Option<u64> {
        self.inner.content_length()
    }

    /// Get the response values of the specified header.
    ///
    /// Multiple values may be returned if the header is present multiple times, depending on the
    /// backend implementation.
    pub fn get_header(&self, header: &str) -> crate::Result<Vec<String>> {
        Ok(self.inner.get_header(header)?)
    }

    /// Block the current thread until getting the full response text.
    ///
    /// Encoding conversion is handled by the backend if possible. Some backends needs extra
    /// features to be enabled to support encoding conversion.
    ///
    /// The maximum size of the response is limited by the
    /// [`crate::ClientBuilder::max_response_buffer_size`] option. If the backend is not able to
    /// receive the response body within the limit, [`crate::Error::ResponseTooLarge`] will be
    /// returned.
    pub fn text(mut self) -> crate::Result<String> {
        Ok(self.inner.text()?)
    }

    /// Block the current thread until getting the full response bytes.
    ///
    /// The maximum size of the response is limited by the
    /// [`crate::ClientBuilder::max_response_buffer_size`] option. If the backend is not able to
    /// receive the response body within the limit, [`crate::Error::ResponseTooLarge`] will be
    /// returned.
    pub fn bytes(mut self) -> crate::Result<Vec<u8>> {
        Ok(AnyBlockingResponse::bytes(&mut *self.inner)?)
    }

    /// Block the current thread until getting the full response bytes, and deserialize into the
    /// given type.
    ///
    /// The maximum size of the response is limited by the
    /// [`crate::ClientBuilder::max_response_buffer_size`] option. If the backend is not able to
    /// receive the response body within the limit, [`crate::Error::ResponseTooLarge`] will be
    /// returned.
    #[cfg(feature = "json")]
    #[cfg_attr(docsrs, doc(cfg(feature = "json")))]
    pub fn json<T: serde::de::DeserializeOwned>(self) -> crate::Result<T> {
        Ok(serde_json::from_slice(&self.bytes()?)?)
    }

    #[doc(hidden)]
    pub fn into_read(self) -> impl io::Read {
        self.inner
    }
}

impl From<Box<dyn AnyBlockingResponse>> for Response {
    fn from(inner: Box<dyn AnyBlockingResponse>) -> Self {
        Self { inner }
    }
}

struct ResponseDebug<'a> {
    inner: &'a dyn AnyBlockingResponse,
}
impl Debug for ResponseDebug<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.inner.describe(f)
    }
}

impl Debug for Response {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BlockingResponse")
            .field("status", &self.status())
            .field("content_length", &self.content_length())
            .field(
                "inner",
                &ResponseDebug {
                    inner: &*self.inner,
                },
            )
            .finish()
    }
}
