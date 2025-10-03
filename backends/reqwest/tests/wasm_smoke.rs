#![cfg(target_arch = "wasm32")]

use nyquest_backend_reqwest::ReqwestBackend;
use nyquest_interface::{client::ClientOptions, r#async::AnyAsyncBackend as _, Method, Request};
use wasm_bindgen_test::wasm_bindgen_test;

const BASE_ADDR: &str = "https://cloudflare-dns.com/";
const USER_AGENT: &str = "reqwest/0.0 nyquest/0 wasm32";
const ACCEPT: &str = "accept";
const DNS_CONTENT_TYPE: &str = "application/dns-json";

#[wasm_bindgen_test]
async fn get_1111_query() {
    let backend = ReqwestBackend;
    let mut options = ClientOptions::default();
    options.base_url = Some(BASE_ADDR.into());
    options.user_agent = Some(USER_AGENT.into());
    options
        .default_headers
        .push((ACCEPT.into(), DNS_CONTENT_TYPE.into()));

    let client = backend.create_async_client(options).await.unwrap();

    let request = Request {
        method: Method::Get,
        relative_uri: "dns-query?name=github.com&type=A".into(),
        additional_headers: vec![],
        body: None,
    };

    let mut response = client.request(request).await.unwrap();
    assert_eq!(response.status(), 200);

    let text = response.as_mut().text().await.unwrap();
    assert!(text.contains(r#""Answer""#));
}

#[wasm_bindgen_test]
async fn post_1111_query() {
    let backend = ReqwestBackend;
    let mut options = ClientOptions::default();
    options.base_url = Some(BASE_ADDR.into());
    options.user_agent = Some(USER_AGENT.into());

    let client = backend.create_async_client(options).await.unwrap();
    let body = b"\xab\xcd\x01\x00\x00\x01\x00\x00\x00\x00\x00\x00\x03www\x07example\x03com\x00\x00\x01\x00\x01";
    let request = Request {
        method: Method::Post,
        relative_uri: "dns-query".into(),
        additional_headers: vec![("accept".into(), "application/dns-message".into())],
        body: Some(nyquest_interface::Body::Bytes {
            content: std::borrow::Cow::Borrowed(body),
            content_type: "application/dns-message".into(),
        }),
    };
    let mut response = client.request(request).await.unwrap();
    assert_eq!(response.status(), 200);

    let text = response.as_mut().text().await.unwrap();
    assert!(text.contains("example"));
}
