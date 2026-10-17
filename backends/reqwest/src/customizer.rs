use std::{fmt, sync::Arc};

/// The customizing function held by a [`Customizer`].
type Hook = Arc<dyn Fn(reqwest::ClientBuilder) -> reqwest::ClientBuilder + Send + Sync>;

/// A client-specific hook that customizes the [`reqwest::ClientBuilder`] of a client.
///
/// Instances are created by [`ReqwestBackend::customize`](crate::ReqwestBackend::customize) or
/// [`ReqwestBackend::with_customizer`](crate::ReqwestBackend::with_customizer) and stored in the
/// backend, so that clients built with
/// [`custom_backend`](https://docs.rs/nyquest/latest/nyquest/struct.ClientBuilder.html#method.custom_backend)
/// carry the customization without affecting the globally registered backend or any other client.
#[derive(Default, Clone)]
pub(crate) struct Customizer {
    f: Option<Hook>,
}

impl fmt::Debug for Customizer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Customizer")
            .field("is_customized", &self.f.is_some())
            .finish()
    }
}

impl Customizer {
    /// Wraps `f` into a [`Customizer`].
    ///
    /// The hook is invoked with a [`reqwest::ClientBuilder`] that already carries nyquest's
    /// built-in configuration, and is expected to return it once the wanted settings have been
    /// applied. Note that a [`reqwest::ClientBuilder`] cannot hold an error: a hook reports
    /// failure by letting [`reqwest::ClientBuilder::build`] reject the resulting configuration.
    pub fn new(
        f: impl Fn(reqwest::ClientBuilder) -> reqwest::ClientBuilder + Send + Sync + 'static,
    ) -> Self {
        Self {
            f: Some(Arc::new(f)),
        }
    }

    /// Applies the customization to `builder`.
    ///
    /// It is a no-op if no hook has been configured.
    pub(crate) fn customize(&self, builder: reqwest::ClientBuilder) -> reqwest::ClientBuilder {
        if let Some(f) = &self.f {
            f(builder)
        } else {
            builder
        }
    }
}
