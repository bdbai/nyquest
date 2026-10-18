use crate::customizer::Customizer;

/// The backend implementation using reqwest.
#[derive(Clone, Debug, Default)]
pub struct ReqwestBackend {
    customizer: Customizer,
}

impl ReqwestBackend {
    /// Creates a backend that applies `customizer` to the [`reqwest::ClientBuilder`] of every
    /// client it creates.
    ///
    /// The resulting backend can be passed to
    /// [`ClientBuilder::custom_backend`](https://docs.rs/nyquest/latest/nyquest/struct.ClientBuilder.html#method.custom_backend)
    /// to build clients that use it.
    fn with_customizer(customizer: Customizer) -> Self {
        Self { customizer }
    }

    /// Creates a backend that applies `customizer` to the [`reqwest::ClientBuilder`] of every client it
    /// creates.
    ///
    /// `customizer` is called once per client, after nyquest has applied the built-in
    /// [`ClientOptions`](nyquest_interface::client::ClientOptions), so that it has the final say
    /// over the resulting client. [`reqwest::ClientBuilder`]'s methods take `self` and return
    /// `Self`, so `customizer` is expected to chain whatever it wants to apply and return the builder:
    ///
    /// ```rust
    /// use nyquest_backend_reqwest::{reqwest, ReqwestBackend};
    ///
    /// let backend = ReqwestBackend::customize(|builder| {
    ///     #[cfg(not(target_arch = "wasm32"))]
    ///     {
    ///         builder
    ///             .pool_max_idle_per_host(0)
    ///             .redirect(reqwest::redirect::Policy::limited(2))
    ///     }
    ///     #[cfg(target_arch = "wasm32")]
    ///     builder
    /// });
    /// ```
    ///
    /// `customizer` runs once per `build_async` / `build_blocking` call, and may be called on different
    /// threads, so it must be `Send + Sync`. It is otherwise a plain closure, and may capture
    /// whatever it needs.
    ///
    /// # Note
    ///
    /// The customizer runs last, so it may override what nyquest has configured, e.g. by
    /// replacing the timeout, the cookie store or the redirect policy. Configurations that
    /// reqwest only validates when the client is built are reported as an error by the `build_*`
    /// call that triggers them.
    pub fn customize(
        customizer: impl Fn(reqwest::ClientBuilder) -> reqwest::ClientBuilder + Send + Sync + 'static,
    ) -> Self {
        Self::with_customizer(Customizer::new(customizer))
    }

    /// The customization configured for this backend, if any.
    pub(crate) fn customizer(&self) -> &Customizer {
        &self.customizer
    }
}
