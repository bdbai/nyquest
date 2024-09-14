use super::{BackendResult, ClientOptions};

pub trait BlockingClient: Send + Sync + 'static {
    fn request(
        &self,
        method: &mut str,
        uri: &str,
        headers: (),
        body: Option<()>,
    ) -> BackendResult<String>;
}

pub trait BlockingBackend: Send + Sync + 'static {
    type BlockingClient: BlockingClient;
    fn create_blocking_client(&self, options: ClientOptions) -> Self::BlockingClient;
}

pub(crate) trait AnyBlockingBackend: Send + Sync + 'static {
    fn create_blocking_client(&self, options: super::ClientOptions) -> Box<dyn BlockingClient>;
}
