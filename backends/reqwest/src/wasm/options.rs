use std::time::Duration;

use nyquest_interface::client::CachingBehavior;

#[derive(Debug, Clone)]
pub(crate) struct WasmOptions {
    pub(crate) request_timeout: Option<Duration>,
    pub(crate) caching_behavior: CachingBehavior,
    pub(crate) use_cookies: bool,
}
