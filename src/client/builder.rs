use nyquest_interface::client::{CachingBehavior, ClientOptions};

#[derive(Debug, Clone, Default)]
pub struct ClientBuilder {
    pub(crate) options: ClientOptions,
}

impl ClientBuilder {
    pub fn base_url(mut self, base_url: impl Into<String>) -> Self {
        self.options.base_url = Some(base_url.into());
        self
    }

    pub fn user_agent(mut self, user_agent: impl Into<String>) -> Self {
        self.options.user_agent = Some(user_agent.into());
        self
    }

    pub fn with_header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.options
            .default_headers
            .push((name.into(), value.into()));
        self
    }

    // TODO: auth

    pub fn no_caching(mut self) -> Self {
        self.options.caching_behavior = CachingBehavior::Disabled;
        self
    }

    pub fn no_proxy(mut self) -> Self {
        self.options.use_default_proxy = false;
        self
    }

    pub fn no_cookies(mut self) -> Self {
        self.options.use_cookies = false;
        self
    }

    pub fn max_response_buffer_size(mut self, size: u64) -> Self {
        self.options.max_response_buffer_size = Some(size);
        self
    }
}
