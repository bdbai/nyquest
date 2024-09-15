use super::CachingBehavior;

#[derive(Debug, Clone, Default)]
pub struct ClientBuilder {
    pub(crate) options: super::ClientOptions,
}

impl ClientBuilder {
    pub fn base_url(&mut self, base_url: impl Into<String>) -> &mut Self {
        self.options.base_url = Some(base_url.into());
        self
    }

    pub fn user_agent(&mut self, user_agent: impl Into<String>) -> &mut Self {
        self.options.user_agent = Some(user_agent.into());
        self
    }

    pub fn no_caching(&mut self) -> &mut Self {
        self.options.caching_behavior = CachingBehavior::Disabled;
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
}
