use nyquest_interface::blocking::{BlockingBackend, BlockingClient, BlockingResponse, Request};
use nyquest_interface::client::{BuildClientError, BuildClientResult, ClientOptions};
use nyquest_interface::{Error as NyquestError, Result as NyquestResult};
use objc2::rc::Retained;
use objc2::runtime::ProtocolObject;
use objc2::AllocAnyThread;
use objc2_foundation::{NSMutableDictionary, NSString, NSURLRequest, NSURL};

use crate::NSUrlSessionBackend;

#[derive(Clone)]
pub struct NSUrlSessionBlockingClient {
    session: Retained<objc2_foundation::NSURLSession>,
    base_url: Option<Retained<NSURL>>,
}
pub struct NSUrlSessionBlockingResponse;

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
                let headers = NSMutableDictionary::alloc();
                let headers = NSMutableDictionary::initWithCapacity(
                    headers,
                    options.default_headers.len() as _,
                );
                for (key, value) in options.default_headers {
                    headers.setObject_forKey(
                        NSString::from_str(&value).as_ref(),
                        &*ProtocolObject::from_retained(NSString::from_str(&key)),
                    );
                }
                config.setHTTPAdditionalHeaders(Some(headers.as_ref()));
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
        unsafe {
            let nsreq = NSURLRequest::alloc();
            let url = NSURL::URLWithString_relativeToURL(
                &*NSString::from_str(&req.relative_uri),
                self.base_url.as_deref(),
            )
            .ok_or(NyquestError::InvalidUrl)?;
            let nsreq = NSURLRequest::initWithURL(nsreq, &*url);
            self.session.dataTaskWithRequest(&nsreq);
        }
        todo!()
    }
}
