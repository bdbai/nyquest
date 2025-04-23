use nyquest_interface::client::{BuildClientError, BuildClientResult, ClientOptions};

use nyquest_interface::{Body, Error as NyquestError, Request, Result as NyquestResult};
use objc2::rc::Retained;
use objc2::AllocAnyThread;
use objc2_foundation::{ns_string, NSData, NSDictionary, NSMutableURLRequest, NSString, NSURL};

#[derive(Clone)]
pub struct NSUrlSessionClient {
    pub(crate) session: Retained<objc2_foundation::NSURLSession>,
    pub(crate) base_url: Option<Retained<NSURL>>,
}

impl NSUrlSessionClient {
    pub(crate) fn create(options: ClientOptions) -> BuildClientResult<Self> {
        let session = unsafe {
            let config = objc2_foundation::NSURLSessionConfiguration::defaultSessionConfiguration();
            if !options.default_headers.is_empty() {
                let keys: Vec<_> = options
                    .default_headers
                    .iter()
                    .map(|(k, _)| NSString::from_str(k))
                    .collect();
                let values: Vec<_> = options
                    .default_headers
                    .iter()
                    .map(|(_, v)| NSString::from_str(v))
                    .collect();
                let dict = NSDictionary::from_retained_objects(
                    &keys.iter().map(|s| &**s).collect::<Vec<_>>(),
                    &values,
                );
                config.setHTTPAdditionalHeaders(Some(
                    Retained::cast_unchecked::<NSDictionary>(dict).as_ref(),
                ));
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
                    _ => todo!("body types"),
                }
            }
            Ok(self.session.dataTaskWithRequest(&nsreq))
        }
    }
}
