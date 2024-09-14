#[derive(Debug, Clone)]
pub struct ClientOptions {
    pub base_url: Option<String>,
    pub caching_behavior: crate::CachingBehavior,
    pub use_default_proxy: bool,
    pub use_cookies: bool,
    pub follow_redirects: bool,
}

impl Default for ClientOptions {
    fn default() -> Self {
        Self {
            base_url: None,
            caching_behavior: crate::CachingBehavior::Default,
            use_default_proxy: true,
            use_cookies: true,
            follow_redirects: true,
        }
    }
}
