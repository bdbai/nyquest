use std::future::Future;

use crate::client::ClientOptions;
use crate::Result;

pub(crate) trait AnyAsyncBackend: Send + Sync + 'static {
    fn create_async_client<'a>(
        &'a self,
        options: ClientOptions,
    ) -> Box<dyn Future<Output = Box<dyn AnyAsyncClient>> + Send + 'a>;
}

pub(crate) trait AnyAsyncClient: Send + Sync + 'static {
    fn request(
        &self,
        method: &str,
        uri: &str,
        headers: (),
        body: Option<()>,
    ) -> Box<dyn Future<Output = Result<String>> + Send>;
}

impl<A> AnyAsyncBackend for A
where
    A: super::backend::AsyncBackend,
{
    fn create_async_client<'a>(
        &'a self,
        options: ClientOptions,
    ) -> Box<dyn Future<Output = Box<dyn AnyAsyncClient>> + Send + 'a> {
        Box::new(async {
            Box::new(super::backend::AsyncBackend::create_async_client(self, options).await) as _
        })
    }
}

impl<A> AnyAsyncClient for A
where
    A: super::backend::AsyncClient,
{
    fn request(
        &self,
        method: &str,
        uri: &str,
        headers: (),
        body: Option<()>,
    ) -> Box<dyn Future<Output = Result<String>> + Send> {
        Box::new(super::backend::AsyncClient::request(
            self, method, uri, headers, body,
        ))
    }
}
