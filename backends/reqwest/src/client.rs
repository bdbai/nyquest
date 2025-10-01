use std::sync::{Arc, OnceLock};

use http::{HeaderMap, HeaderName, HeaderValue};
use nyquest_interface::{client::ClientOptions, Result as NyquestResult};
use reqwest::Client;
use tokio::runtime::Runtime;
use url::Url;

use crate::error::{ReqwestBackendError, Result};

#[derive(Clone)]
pub struct ReqwestClient {
    pub(crate) client: Client,
    pub(crate) base_url: Option<Url>,
    pub(crate) max_response_buffer_size: Option<u64>,
    pub(crate) managed_runtime: Arc<OnceLock<Runtime>>,
}

impl ReqwestClient {
    pub fn new(options: ClientOptions) -> NyquestResult<Self> {
        let client = build_reqwest_client(&options)?;

        let base_url = if let Some(ref base_url_str) = options.base_url {
            Some(Url::parse(base_url_str).map_err(|_| {
                nyquest_interface::Error::Io(std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    format!("Invalid base URL: {}", base_url_str),
                ))
            })?)
        } else {
            None
        };

        Ok(Self {
            client,
            base_url,
            max_response_buffer_size: options.max_response_buffer_size,
            managed_runtime: Arc::new(OnceLock::new()),
        })
    }
}

pub fn build_reqwest_client(options: &ClientOptions) -> Result<Client> {
    let mut builder = Client::builder();

    if let Some(user_agent) = &options.user_agent {
        builder = builder.user_agent(user_agent);
    }
    if !options.use_default_proxy {
        builder = builder.no_proxy();
    }
    let default_headers: Result<HeaderMap> = options
        .default_headers
        .iter()
        .map(|(k, v)| {
            Ok::<_, ReqwestBackendError>((
                HeaderName::from_bytes(k.as_bytes())
                    .map_err(|_| ReqwestBackendError::InvalidHeaderName(k.into()))?,
                HeaderValue::from_str(v)
                    .map_err(|_| ReqwestBackendError::InvalidHeaderValue(v.into()))?,
            ))
        })
        .collect();
    builder = builder
        .default_headers(default_headers?)
        .cookie_store(options.use_cookies)
        .redirect(if options.follow_redirects {
            reqwest::redirect::Policy::default()
        } else {
            reqwest::redirect::Policy::none()
        });
    #[cfg(any(
        feature = "default-tls",
        feature = "native-tls",
        feature = "rustls-tls-minimal",
    ))]
    {
        builder = builder.danger_accept_invalid_certs(options.ignore_certificate_errors);
    }

    if let Some(timeout) = options.request_timeout {
        builder = builder.timeout(timeout);
    }

    builder.build().map_err(ReqwestBackendError::Reqwest)
}
