use std::io;

use nyquest_interface::client::{CachingBehavior, ClientOptions};
use windows::core::{h, HSTRING};
use windows::Web::Http::Filters::{
    HttpBaseProtocolFilter, HttpCacheReadBehavior, HttpCacheWriteBehavior, HttpCookieUsageBehavior,
};
use windows::Web::Http::HttpClient;

use crate::request::is_header_name_content_related;

#[derive(Clone)]
pub struct WinrtClient {
    pub(crate) base_url: Option<HSTRING>,
    pub(crate) max_response_buffer_size: Option<u64>,
    pub(crate) client: HttpClient,
    pub(crate) default_content_headers: Vec<(HSTRING, HSTRING)>,
}

impl WinrtClient {
    pub fn create(options: ClientOptions) -> io::Result<Self> {
        let base_url = options.base_url.as_ref().map(HSTRING::from);
        let filter = HttpBaseProtocolFilter::new()?;
        filter.SetAutomaticDecompression(true)?;
        if options.caching_behavior == CachingBehavior::Disabled {
            let cache_control = filter.CacheControl()?;
            cache_control.SetReadBehavior(HttpCacheReadBehavior::NoCache)?;
            cache_control.SetWriteBehavior(HttpCacheWriteBehavior::NoCache)?;
        }
        if !options.use_default_proxy {
            filter.SetUseProxy(false)?;
        }
        if !options.use_cookies {
            filter.SetCookieUsageBehavior(HttpCookieUsageBehavior::NoCookies)?;
        }
        let client = HttpClient::Create(&filter)?;
        if let Some(user_agent) = &options.user_agent {
            client
                .DefaultRequestHeaders()?
                .Append(h!("user-agent"), &HSTRING::from(user_agent))?;
        }
        let mut default_content_headers = vec![];
        for (name, value) in options.default_headers {
            if is_header_name_content_related(&name) {
                default_content_headers.push((HSTRING::from(name), HSTRING::from(value)));
            } else {
                client
                    .DefaultRequestHeaders()?
                    .TryAppendWithoutValidation(&HSTRING::from(name), &HSTRING::from(value))?;
            }
        }
        // TODO: options
        Ok(Self {
            base_url,
            max_response_buffer_size: options.max_response_buffer_size,
            client,
            default_content_headers,
        })
    }
}
