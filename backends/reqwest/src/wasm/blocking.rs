use std::io::Read;

use nyquest_interface::blocking::{BlockingBackend, BlockingClient, BlockingResponse};

#[derive(Clone)]
pub struct DummyBlockingClient {
    /// The blocking client cannot send requests on WASM, but its cookie API works through the
    /// synchronous `document.cookie` API.
    #[cfg(feature = "cookies")]
    inner: crate::client::ReqwestClient,
}
pub struct DummyBlockingResponse;

fn bail_unimplemented() -> ! {
    unimplemented!("blocking backend should not be used in wasm32 target");
}

impl Read for DummyBlockingResponse {
    fn read(&mut self, _buf: &mut [u8]) -> std::io::Result<usize> {
        bail_unimplemented()
    }
}

impl BlockingResponse for DummyBlockingResponse {
    fn status(&self) -> u16 {
        bail_unimplemented()
    }

    fn content_length(&self) -> Option<u64> {
        bail_unimplemented()
    }

    fn get_header(&self, _header: &str) -> nyquest_interface::Result<Vec<String>> {
        bail_unimplemented()
    }

    fn text(&mut self) -> nyquest_interface::Result<String> {
        bail_unimplemented()
    }

    fn bytes(&mut self) -> nyquest_interface::Result<Vec<u8>> {
        bail_unimplemented()
    }
}

impl BlockingClient for DummyBlockingClient {
    type Response = DummyBlockingResponse;

    fn request(
        &self,
        _req: nyquest_interface::blocking::Request,
    ) -> nyquest_interface::Result<Self::Response> {
        bail_unimplemented()
    }

    #[cfg(feature = "blocking-ws")]
    fn connect_websocket(
        &self,
        _uri: std::borrow::Cow<'static, str>,
        _protocols: Vec<std::borrow::Cow<'static, str>>,
    ) -> nyquest_interface::WebSocketResult<nyquest_interface::blocking::websocket::WebSocket> {
        bail_unimplemented()
    }

    #[cfg(feature = "cookies")]
    fn add_cookie(
        &self,
        name: std::borrow::Cow<'static, str>,
        value: std::borrow::Cow<'static, str>,
        url: std::borrow::Cow<'static, str>,
    ) -> nyquest_interface::Result<()> {
        self.inner.add_cookie_sync(&name, &value, &url)
    }

    #[cfg(feature = "cookies")]
    fn get_cookies_for(
        &self,
        url: std::borrow::Cow<'static, str>,
    ) -> nyquest_interface::Result<Vec<nyquest_interface::Cookie>> {
        self.inner.get_cookies_for_sync(&url)
    }
}

impl BlockingBackend for crate::ReqwestBackend {
    type BlockingClient = DummyBlockingClient;

    fn create_blocking_client(
        &self,
        options: nyquest_interface::client::ClientOptions,
    ) -> nyquest_interface::Result<Self::BlockingClient> {
        #[cfg(feature = "cookies")]
        {
            Ok(DummyBlockingClient {
                inner: crate::client::ReqwestClient::new(options, self.customizer())?,
            })
        }
        #[cfg(not(feature = "cookies"))]
        {
            let _ = options;
            bail_unimplemented()
        }
    }
}
