use std::borrow::Cow;
use std::sync::LazyLock;

use nyquest_interface::client::{
    BuildClientError, BuildClientResult, CachingBehavior, ClientOptions,
};

use nyquest_interface::{Body, Error as NyquestError, Request, Result as NyquestResult};
use objc2::rc::Retained;
use objc2::AllocAnyThread;
use objc2_foundation::{
    ns_string, NSCharacterSet, NSData, NSDictionary, NSMutableCharacterSet, NSMutableURLRequest,
    NSString, NSURLRequestCachePolicy, NSUTF8StringEncoding, NSURL,
};

#[derive(Clone)]
pub struct NSUrlSessionClient {
    pub(crate) session: Retained<objc2_foundation::NSURLSession>,
    pub(crate) base_url: Option<Retained<NSURL>>,
}

impl NSUrlSessionClient {
    pub(crate) fn create(options: ClientOptions) -> BuildClientResult<Self> {
        let session = unsafe {
            let config = objc2_foundation::NSURLSessionConfiguration::defaultSessionConfiguration();
            if !options.use_default_proxy {
                config.setConnectionProxyDictionary(Some(&*NSDictionary::new()));
            }
            if !options.default_headers.is_empty() || options.user_agent.is_some() {
                let headers = options
                    .default_headers
                    .iter()
                    .map(|(k, v)| (&**k, &**v))
                    .chain(options.user_agent.as_deref().map(|ua| ("user-agent", ua)));
                let keys: Vec<_> = headers
                    .clone()
                    .map(|(k, _)| NSString::from_str(k))
                    .collect();
                let values: Vec<_> = headers.map(|(_, v)| NSString::from_str(v)).collect();
                let dict = NSDictionary::from_retained_objects(
                    &keys.iter().map(|s| &**s).collect::<Vec<_>>(),
                    &values,
                );
                config.setHTTPAdditionalHeaders(Some(
                    Retained::cast_unchecked::<NSDictionary>(dict).as_ref(),
                ));
                if options.caching_behavior == CachingBehavior::Disabled {
                    config.setRequestCachePolicy(
                        NSURLRequestCachePolicy::ReloadIgnoringLocalCacheData,
                    );
                }
            }
            // TODO: set options
            objc2_foundation::NSURLSession::sessionWithConfiguration(&config)
        };
        let base_url = options
            .base_url
            .map(|url| unsafe {
                NSURL::URLWithString(&NSString::from_str(&url))
                    .ok_or(BuildClientError::BackendError(NyquestError::InvalidUrl))
            })
            .transpose()?;
        Ok(Self { session, base_url })
    }

    pub(crate) fn build_data_task<S>(
        &self,
        req: Request<S>,
    ) -> NyquestResult<Retained<objc2_foundation::NSURLSessionDataTask>> {
        let nsreq = NSMutableURLRequest::alloc();
        unsafe {
            let url = NSURL::URLWithString_relativeToURL(
                &NSString::from_str(&req.relative_uri),
                self.base_url.as_deref(),
            )
            .ok_or(NyquestError::InvalidUrl)?;
            let nsreq = NSMutableURLRequest::initWithURL(nsreq, &url);
            nsreq.setHTTPMethod(&NSString::from_str(&req.method));
            for (name, value) in &req.additional_headers {
                nsreq.setValue_forHTTPHeaderField(
                    Some(&NSString::from_str(value)),
                    &NSString::from_str(name),
                );
            }
            if let Some(body) = req.body {
                match body {
                    Body::Bytes {
                        content,
                        content_type,
                    } => {
                        nsreq.setValue_forHTTPHeaderField(
                            Some(&NSString::from_str(&content_type)),
                            ns_string!("content-type"),
                        );
                        nsreq.setHTTPBody(Some(&NSData::from_vec(content.into())));
                    }
                    Body::Form { fields } => {
                        static FORM_URLENCODER: LazyLock<FormUrlEncoder> =
                            LazyLock::new(FormUrlEncoder::new);
                        let data = FORM_URLENCODER.encode_fields(&fields);
                        nsreq.setValue_forHTTPHeaderField(
                            Some(ns_string!("application/x-www-form-urlencoded")),
                            ns_string!("content-type"),
                        );
                        nsreq.setHTTPBody(Some(&data));
                    }
                    #[cfg(feature = "multipart")]
                    Body::Multipart { parts } => {
                        use crate::multipart::{
                            generate_multipart_body, generate_multipart_boundary,
                        };
                        let boundary = generate_multipart_boundary();
                        let content_type = format!("multipart/form-data; boundary={}", boundary);
                        nsreq.setValue_forHTTPHeaderField(
                            Some(&NSString::from_str(&content_type)),
                            ns_string!("content-type"),
                        );
                        nsreq.setHTTPBody(Some(&generate_multipart_body(&boundary, parts)));
                    }
                    _ => todo!("body types"),
                }
            }
            Ok(self.session.dataTaskWithRequest(&nsreq))
        }
    }
}

struct FormUrlEncoder(Retained<NSCharacterSet>);
impl FormUrlEncoder {
    fn new() -> Self {
        unsafe {
            let set = NSMutableCharacterSet::alphanumericCharacterSet();
            set.addCharactersInString(ns_string!("-._* "));
            Self(set.downcast().unwrap())
        }
    }
    fn encode(&self, s: &str) -> Retained<NSString> {
        unsafe {
            NSString::from_str(s)
                .stringByAddingPercentEncodingWithAllowedCharacters(&self.0)
                .unwrap_or_default()
                .stringByReplacingOccurrencesOfString_withString(ns_string!(" "), ns_string!("+"))
        }
    }
    fn encode_fields(&self, fields: &[(Cow<'static, str>, Cow<'static, str>)]) -> Retained<NSData> {
        let Some(((first_key, first_val), fields)) = fields.split_first() else {
            return NSData::new();
        };
        let mut encoded = self
            .encode(first_key)
            .stringByAppendingString(ns_string!("="))
            .stringByAppendingString(&self.encode(first_val));
        for (key, val) in fields {
            encoded = encoded
                .stringByAppendingString(ns_string!("&"))
                .stringByAppendingString(&self.encode(key))
                .stringByAppendingString(ns_string!("="))
                .stringByAppendingString(&self.encode(val));
        }
        let data = unsafe { encoded.dataUsingEncoding(NSUTF8StringEncoding) };
        data.unwrap_or_default()
    }
}

unsafe impl Send for FormUrlEncoder {}
unsafe impl Sync for FormUrlEncoder {}
