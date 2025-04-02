use nyquest_interface::blocking::{BlockingBackend, BlockingClient, BlockingResponse, Request};
use nyquest_interface::client::{BuildClientError, BuildClientResult, ClientOptions};
use nyquest_interface::{Body, Error as NyquestError, Result as NyquestResult};
use objc2::rc::Retained;
use objc2::runtime::ProtocolObject;
use objc2::AllocAnyThread;
use objc2_foundation::{ns_string, NSData, NSDictionary, NSMutableURLRequest, NSString, NSURL};

mod datatask_delegate;

use crate::NSUrlSessionBackend;

#[derive(Clone)]
pub struct NSUrlSessionBlockingClient {
    session: Retained<objc2_foundation::NSURLSession>,
    base_url: Option<Retained<NSURL>>,
}
pub struct NSUrlSessionBlockingResponse {
    task: Retained<objc2_foundation::NSURLSessionDataTask>,
}

impl std::io::Read for NSUrlSessionBlockingResponse {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        todo!()
    }
}

impl BlockingResponse for NSUrlSessionBlockingResponse {
    fn status(&self) -> u16 {
        todo!()
    }

    fn content_length(&self) -> Option<u64> {
        todo!()
    }

    fn get_header(&self, header: &str) -> nyquest_interface::Result<Vec<String>> {
        todo!()
    }

    fn text(&mut self) -> nyquest_interface::Result<String> {
        todo!()
    }

    fn bytes(&mut self) -> nyquest_interface::Result<Vec<u8>> {
        todo!()
    }
}

impl BlockingBackend for NSUrlSessionBackend {
    type BlockingClient = NSUrlSessionBlockingClient;

    fn create_blocking_client(
        &self,
        options: ClientOptions,
    ) -> BuildClientResult<Self::BlockingClient> {
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
                    &*keys.iter().map(|s| &**s).collect::<Vec<_>>(),
                    &*values,
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
                NSURL::URLWithString(&*NSString::from_str(&url))
                    .ok_or(BuildClientError::BackendError(NyquestError::InvalidUrl))
            })
            .transpose()?;
        Ok(NSUrlSessionBlockingClient { session, base_url })
    }
}

impl BlockingClient for NSUrlSessionBlockingClient {
    type Response = NSUrlSessionBlockingResponse;

    fn request(&self, req: Request) -> nyquest_interface::Result<Self::Response> {
        let nsreq = NSMutableURLRequest::alloc();
        let task = unsafe {
            let url = NSURL::URLWithString_relativeToURL(
                &*NSString::from_str(&req.relative_uri),
                self.base_url.as_deref(),
            )
            .ok_or(NyquestError::InvalidUrl)?;
            let nsreq = NSMutableURLRequest::initWithURL(nsreq, &*url);
            nsreq.setHTTPMethod(&*NSString::from_str(&req.method));
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
            // TODO: use delegate to receive response headers
            let task = self.session.dataTaskWithRequest(&nsreq);
            let delegate = datatask_delegate::BlockingDataTaskDelegate::new();
            task.setDelegate(Some(ProtocolObject::from_ref(&*delegate)));
            task.resume();
            task
        };
        Ok(NSUrlSessionBlockingResponse { task })
    }
}
