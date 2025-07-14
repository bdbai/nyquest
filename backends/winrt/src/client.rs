use std::io;
use std::time::Duration;

use nyquest_interface::client::{CachingBehavior, ClientOptions};
use windows::core::{h, HSTRING};
use windows::Security::Cryptography::Certificates::ChainValidationResult;
use windows::Web::Http::Filters::{
    HttpBaseProtocolFilter, HttpCacheReadBehavior, HttpCacheWriteBehavior, HttpCookieUsageBehavior,
};
use windows::Web::Http::HttpClient;

use crate::request::is_header_name_content_related;

#[derive(Clone)]
pub struct WinrtClient {
    pub(crate) base_url: Option<HSTRING>,
    pub(crate) max_response_buffer_size: Option<u64>,
    pub(crate) request_timeout: Option<Duration>,
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
        if options.ignore_certificate_errors {
            let ignorables = filter.IgnorableServerCertificateErrors()?;
            ignorables.Clear()?;
            for i in 1..=13 {
                ignorables.Append(ChainValidationResult(i)).ok();
            }
        }
        if !options.follow_redirects {
            filter.SetAllowAutoRedirect(false)?;
        }
        let client = HttpClient::Create(&filter)?;
        if let Some(user_agent) = &options.user_agent {
            client
                .DefaultRequestHeaders()?
                .Append(h!("user-agent"), &HSTRING::from(user_agent))?;
        }
        let mut default_content_headers = vec![];
        for (name, value) in options.default_headers {
            if name.eq_ignore_ascii_case("content-type") {
                // If a request has a body, the content-type value is required from the user.
                // Otherwise if there is no body, the content-type header will never be sent.
                // So we can safely ignore the default content-type header.
            } else if is_header_name_content_related(&name) {
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
            request_timeout: options.request_timeout,
            client,
            default_content_headers,
        })
    }
}
