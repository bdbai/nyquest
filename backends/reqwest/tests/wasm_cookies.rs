#![cfg(all(target_arch = "wasm32", feature = "cookies"))]

use std::sync::Arc;

use nyquest_interface::{
    blocking::{AnyBlockingBackend as _, AnyBlockingClient},
    client::ClientOptions,
};
use wasm_bindgen_test::wasm_bindgen_test;

wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);

const URL: &str = "https://example.com/path/page";

fn new_client(options: ClientOptions) -> Arc<dyn AnyBlockingClient> {
    nyquest_backend_reqwest::ReqwestBackend::default()
        .create_blocking_client(options)
        .expect("create blocking client")
}

#[wasm_bindgen_test]
fn add_and_get_cookie() {
    let client = new_client(ClientOptions::default());

    client
        .add_cookie(
            "NYQUEST_WASM_COOKIE".into(),
            "mock_cookie_value".into(),
            URL.into(),
        )
        .unwrap();

    let cookies = client.get_cookies_for(URL.into()).unwrap();
    let cookie = cookies
        .iter()
        .find(|cookie| cookie.name == "NYQUEST_WASM_COOKIE")
        .expect("cookie should be visible through document.cookie");
    assert_eq!(cookie.value, "mock_cookie_value");
}

#[wasm_bindgen_test]
fn special_characters_round_trip() {
    const VALUE: &str = "你好，世界！; = 100%";

    let client = new_client(ClientOptions::default());

    client
        .add_cookie(
            "NYQUEST_WASM_UNICODE_COOKIE".into(),
            VALUE.into(),
            URL.into(),
        )
        .unwrap();

    let cookies = client.get_cookies_for(URL.into()).unwrap();
    let cookie = cookies
        .iter()
        .find(|cookie| cookie.name == "NYQUEST_WASM_UNICODE_COOKIE")
        .expect("cookie should be visible through document.cookie");
    assert_eq!(cookie.value, VALUE);
}

#[wasm_bindgen_test]
fn cookies_disabled_is_a_no_op() {
    let mut options = ClientOptions::default();
    options.use_cookies = false;

    let client = new_client(options);

    client
        .add_cookie(
            "NYQUEST_WASM_DISABLED_COOKIE".into(),
            "value".into(),
            URL.into(),
        )
        .unwrap();

    let cookies = client.get_cookies_for(URL.into()).unwrap();
    assert!(cookies.is_empty());
}
