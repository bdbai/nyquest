use std::time::Duration;

#[derive(Debug, Clone)]
pub(crate) struct WasmOptions {
    pub(crate) request_timeout: Option<Duration>,
    pub(crate) caching_behavior: CachingBehavior,
    pub(crate) use_cookies: bool,
}
