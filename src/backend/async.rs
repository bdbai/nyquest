use core::future::Future;

use super::BackendResult;

pub trait AsyncClient: Send + Sync + 'static {
    type RequestFut: Future<Output = BackendResult<String>> + Send;
    fn request(
        &self,
        method: &mut str,
        uri: &str,
        headers: (),
        body: Option<()>,
    ) -> Self::RequestFut;
    // TODO: fn request_with_progress
}

pub(crate) trait AnyAsyncClient: Send + Sync + 'static {
    fn request(
        &self,
        method: &mut str,
        uri: &str,
        headers: (),
        body: Option<()>,
    ) -> Box<dyn Future<Output = BackendResult<String>> + Send>;
}

pub trait AsyncBackend: Send + Sync + 'static {
    type AsyncClient: AsyncClient;
    fn create_async_client(
        &self,
        options: super::ClientOptions,
    ) -> impl Future<Output = Self::AsyncClient> + Send;
}

pub(crate) trait AnyAsyncBackend: Send + Sync + 'static {
    fn create_async_client<'a>(
        &'a self,
        options: super::ClientOptions,
    ) -> Box<dyn Future<Output = Box<dyn AnyAsyncClient>> + Send + 'a>;
}

impl<A> AnyAsyncClient for A
where
    A: AsyncClient,
{
    fn request(
        &self,
        method: &mut str,
        uri: &str,
        headers: (),
        body: Option<()>,
    ) -> Box<dyn Future<Output = BackendResult<String>> + Send> {
        Box::new(AsyncClient::request(self, method, uri, headers, body))
    }
}

impl<A> AnyAsyncBackend for A
where
    A: AsyncBackend,
{
    fn create_async_client<'a>(
        &'a self,
        options: super::ClientOptions,
    ) -> Box<dyn Future<Output = Box<dyn AnyAsyncClient>> + Send + 'a> {
        Box::new(async { Box::new(AsyncBackend::create_async_client(self, options).await) as _ })
    }
}
