use thiserror::Error;

#[derive(Debug, Clone, Default)]
pub struct ClientBuilder {
    options: crate::backend::ClientOptions,
}

#[derive(Debug, Error)]
pub enum ClientBuilderError {
    #[error("No backend registered")]
    NoBackend,
    #[error("Invalid base URL")]
    InvalidBaseUrl,
}

impl ClientBuilder {
    pub fn base_url(&mut self, base_url: impl Into<String>) -> &mut Self {
        self.options.base_url = Some(base_url.into());
        self
    }

    pub fn no_caching(&mut self) -> &mut Self {
        self.options.caching_behavior = crate::CachingBehavior::Disabled;
        self
    }

    pub fn no_proxy(&mut self) -> &mut Self {
        self.options.use_default_proxy = false;
        self
    }

    pub fn no_cookies(&mut self) -> &mut Self {
        self.options.use_cookies = false;
        self
    }

    #[cfg(feature = "async")]
    pub fn build_async(self) -> crate::Client {
        todo!()
    }

    #[cfg(feature = "blocking")]
    pub fn build_blocking(self) -> crate::BlockingClient {
        todo!()
    }
}
