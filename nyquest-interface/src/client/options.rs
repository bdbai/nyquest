#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum CachingBehavior {
    Disabled,
    BestEffort,
    #[default]
    Default,
}

#[derive(Debug, Clone)]
pub struct ClientOptions {
    pub base_url: Option<String>,
    pub user_agent: Option<String>,
    pub default_headers: Vec<(String, String)>,
    pub caching_behavior: CachingBehavior,
    pub use_default_proxy: bool,
    pub use_cookies: bool,
    pub follow_redirects: bool,
    pub max_response_buffer_size: Option<u64>,
    // TODO: timeouts
}

impl Default for ClientOptions {
    fn default() -> Self {
        Self {
            base_url: None,
            user_agent: None,
            default_headers: vec![],
            caching_behavior: CachingBehavior::Default,
            use_default_proxy: true,
            use_cookies: true,
            follow_redirects: true,
            max_response_buffer_size: None,
        }
    }
}
