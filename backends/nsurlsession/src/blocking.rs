use nyquest_interface::blocking::{BlockingBackend, BlockingClient, Request};
use nyquest_interface::client::ClientOptions;
use nyquest_interface::{Error as NyquestError, Result as NyquestResult};

use crate::NSUrlSessionBackend;

pub struct NSUrlSessionBlockingClient {
    session: objc2_foundation::NSURLSession,
}
pub struct NSUrlSessionBlockingResponse;

impl BlockingBackend for NSUrlSessionBackend {
    type BlockingClient = NSUrlSessionBlockingClient;

    fn create_blocking_client(
        &self,
        options: ClientOptions,
    ) -> NyquestResult<Self::BlockingClient> {
        Ok(NSUrlSessionBlockingClient { session: todo!() })
    }
}
