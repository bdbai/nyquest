use std::{fmt, sync::Arc};

#[cfg(feature = "blocking")]
use crate::blocking::{
    create_customizer as create_blocking_customizer, Customizer as BlockingCustomizer,
};
#[cfg(feature = "async")]
use crate::r#async::{create_customizer as create_async_customizer, Customizer as AsyncCustomizer};
use crate::{custom::NoopHandleCustomizer, HandleCustomizer};

/// The backend implementation using libcurl.
#[derive(Clone)]
pub struct CurlBackend {
    #[cfg(feature = "async")]
    pub(crate) async_customizer: AsyncCustomizer,
    #[cfg(feature = "blocking")]
    pub(crate) blocking_customizer: BlockingCustomizer,
}

impl CurlBackend {
    /// Creates a backend that applies `customizer` to every easy handle it creates.
    ///
    /// `customizer` receives the easy handle for each request after nyquest has applied the
    /// built-in [`ClientOptions`](nyquest_interface::client::ClientOptions), and may set any
    /// `CURLOPT`, including ones that nyquest has no corresponding option for. It also receives the
    /// request being made, so that an option can depend on what is being requested.
    ///
    /// ```rust
    /// use nyquest_backend_curl::{curl_sys, CurlBackend, CurlHandle, HandleCustomizer};
    /// use nyquest_interface::Request;
    ///
    /// struct Customizer;
    ///
    /// impl HandleCustomizer for Customizer {
    ///     fn customize_handle_with_request<S>(
    ///         &self,
    ///         mut handle: CurlHandle<'_>,
    ///         request: &Request<S>,
    ///     ) -> Result<(), nyquest_backend_curl::CurlCodeContext> {
    ///         // Safety: `CURLOPT_TCP_KEEPALIVE` takes a `long`.
    ///         unsafe { handle.setopt_long(curl_sys::CURLOPT_TCP_KEEPALIVE, 1)? };
    ///         if request.method == nyquest_interface::Method::Post {
    ///             // Safety: `CURLOPT_REFERER` takes a C string.
    ///             unsafe { handle.setopt_ptr_raw(curl_sys::CURLOPT_REFERER, c"custom-referrer".as_ptr())? };
    ///         }
    ///         Ok(())
    ///     }
    /// }
    ///
    /// let backend = CurlBackend::customize(Customizer);
    /// ```
    ///
    /// The same customizer serves both the blocking and the asynchronous client. A client only ever calls
    /// the half it drives requests with, so the shape of the returned future never changes what a
    /// blocking request does.
    ///
    /// # Safety
    ///
    /// This is a thin wrapper around `curl_easy_setopt`, which trusts the caller completely.
    /// `customizer` must therefore ensure that, for every request:
    ///
    /// - each option is passed a value of the kind that option reads, as documented by libcurl.
    ///   The setters on [`crate::CurlHandle`] are `unsafe` for the same reason, and cannot check this for
    ///   the caller: an option number does not say whether it is read as a number, followed as a
    ///   pointer, or called as a function pointer;
    /// - every pointer it hands to libcurl stays valid until the request completes. The owning
    ///   setters take care of this by moving the data into the handle;
    /// - it does not break nyquest's invariants, e.g. by replacing the write/header callbacks or
    ///   the data pointers nyquest installs, or by freeing the handle;
    /// - it does not block when it is called for a blocking request: that call runs synchronously
    ///   on the thread driving the transfer.
    ///
    /// Breaking any of these results in undefined behavior rather than an error. Errors returned by
    /// `customizer` are reported as [`Error::Io`](nyquest_interface::Error::Io) for the request
    /// being made.
    ///
    /// Note that a customization is applied to every request of every client built from this
    /// backend, and may be invoked concurrently on several threads. Refer to the option's
    /// documentation in libcurl when in doubt.
    pub fn customize(customizer: impl HandleCustomizer + Send + Sync + 'static) -> Self {
        let customizer = Arc::new(customizer);
        #[cfg(feature = "async")]
        let async_customizer = create_async_customizer(customizer.clone());
        #[cfg(feature = "blocking")]
        let blocking_customizer = create_blocking_customizer(customizer);
        Self {
            #[cfg(feature = "async")]
            async_customizer,
            #[cfg(feature = "blocking")]
            blocking_customizer,
        }
    }
}

impl Default for CurlBackend {
    fn default() -> Self {
        Self::customize(NoopHandleCustomizer)
    }
}

impl fmt::Debug for CurlBackend {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CurlBackend").finish()
    }
}
