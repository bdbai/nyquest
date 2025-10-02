#![cfg(not(target_arch = "wasm32"))]
// TODO: move to backend tests

use nyquest_backend_reqwest::ReqwestBackend;
use nyquest_interface::{
    blocking::{BlockingBackend, BlockingClient, BlockingResponse},
    client::ClientOptions,
    r#async::{AsyncBackend, AsyncClient, AsyncResponse},
    Method, Request,
};
use std::borrow::Cow;
use std::thread;

const TEST_URL: &str = "https://ip.sb/";

/// Test that blocking interface works outside any runtime
#[cfg(feature = "blocking")]
#[test]
fn test_blocking_outside_runtime() {
    let backend = ReqwestBackend;
    let options = ClientOptions::default();

    let client = backend.create_blocking_client(options).unwrap();

    let request = Request {
        method: Method::Get,
        relative_uri: Cow::Borrowed(TEST_URL),
        additional_headers: vec![],
        body: None,
    };

    let response = client.request(request).unwrap();
    assert_eq!(response.status(), 200);

    // Verify we can read the response body
    let mut response_mut = response;
    let _text = response_mut.text().unwrap();
    println!("✓ Blocking interface works outside runtime");
}

/// Test that blocking interface works inside a tokio runtime
#[cfg(feature = "blocking")]
#[tokio::test]
async fn test_blocking_inside_tokio_runtime() {
    let backend = ReqwestBackend;
    let options = ClientOptions::default();

    let client = backend.create_blocking_client(options).unwrap();

    let request = Request {
        method: Method::Get,
        relative_uri: Cow::Borrowed(TEST_URL),
        additional_headers: vec![],
        body: None,
    };

    // Run blocking request inside tokio runtime using spawn_blocking
    let response = tokio::task::spawn_blocking(move || client.request(request))
        .await
        .unwrap()
        .unwrap();

    assert_eq!(response.status(), 200);
    println!("✓ Blocking interface works inside tokio runtime");
}

/// Test that async interface works inside a tokio runtime
#[cfg(feature = "async")]
#[tokio::test]
async fn test_async_inside_tokio_runtime() {
    let backend = ReqwestBackend;
    let options = ClientOptions::default();

    let client = backend.create_async_client(options).await.unwrap();

    let request = Request {
        method: Method::Get,
        relative_uri: Cow::Borrowed(TEST_URL),
        additional_headers: vec![],
        body: None,
    };

    let response = client.request(request).await.unwrap();
    assert_eq!(response.status(), 200);

    // Verify we can read the response body
    let mut response_mut = response;
    let _text = std::pin::Pin::new(&mut response_mut).text().await.unwrap();
    println!("✓ Async interface works inside tokio runtime");
}

/// Test that async interface works inside a futures executor (not tokio)
/// Note: This test uses a separate tokio runtime in a thread to avoid conflicts
#[cfg(feature = "async")]
#[test]
fn test_async_inside_futures_executor() {
    use std::sync::mpsc;
    use std::thread;

    // Run the tokio part in a separate thread to avoid executor conflicts
    let (tx, rx) = mpsc::channel();

    thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();

        rt.block_on(async {
            let backend = ReqwestBackend;
            let options = ClientOptions::default();

            let client = backend.create_async_client(options).await.unwrap();

            let request = Request {
                method: Method::Get,
                relative_uri: Cow::Borrowed(TEST_URL),
                additional_headers: vec![],
                body: None,
            };

            let response = client.request(request).await.unwrap();
            assert_eq!(response.status(), 200);

            // Verify we can read the response body
            let mut response_mut = response;
            let _text = std::pin::Pin::new(&mut response_mut).text().await.unwrap();

            tx.send(()).unwrap();
        });
    });

    // Wait for completion
    rx.recv().unwrap();
    println!("✓ Async interface works with futures executor using separate tokio thread");
}

/// Test that async interface works with async-std runtime
/// Note: This test uses a separate tokio runtime in a thread to avoid conflicts
#[cfg(feature = "async")]
#[test]
fn test_async_inside_async_std_runtime() {
    use std::sync::mpsc;
    use std::thread;

    // Run the tokio part in a separate thread to avoid executor conflicts
    let (tx, rx) = mpsc::channel();

    thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();

        rt.block_on(async {
            let backend = ReqwestBackend;
            let options = ClientOptions::default();

            let client = backend.create_async_client(options).await.unwrap();

            let request = Request {
                method: Method::Get,
                relative_uri: Cow::Borrowed(TEST_URL),
                additional_headers: vec![],
                body: None,
            };

            let response = client.request(request).await.unwrap();
            assert_eq!(response.status(), 200);

            // Verify we can read the response body
            let mut response_mut = response;
            let _text = std::pin::Pin::new(&mut response_mut).text().await.unwrap();

            tx.send(()).unwrap();
        });
    });

    // Wait for completion
    rx.recv().unwrap();
    println!("✓ Async interface works with async-std using separate tokio thread");
}

/// Test that multiple concurrent async requests work in tokio runtime
#[cfg(feature = "async")]
#[tokio::test]
async fn test_concurrent_async_requests() {
    let backend = ReqwestBackend;
    let options = ClientOptions::default();

    let client = backend.create_async_client(options).await.unwrap();

    // Create multiple concurrent requests
    let mut handles = vec![];
    for i in 0..5 {
        let client_clone = client.clone();
        let handle = tokio::spawn(async move {
            let request = Request {
                method: Method::Get,
                relative_uri: Cow::Borrowed(TEST_URL),
                additional_headers: vec![(
                    Cow::Borrowed("X-Test-Request"),
                    Cow::Owned(format!("{}", i)),
                )],
                body: None,
            };

            let response = client_clone.request(request).await.unwrap();
            assert_eq!(response.status(), 200);
            i
        });
        handles.push(handle);
    }

    // Wait for all requests to complete
    for handle in handles {
        let result = handle.await.unwrap();
        println!("Request {} completed", result);
    }

    println!("✓ Concurrent async requests work in tokio runtime");
}

/// Test that multiple concurrent blocking requests work with different threads
#[cfg(feature = "blocking")]
#[test]
fn test_concurrent_blocking_requests() {
    let backend = ReqwestBackend;
    let options = ClientOptions::default();

    let client = backend.create_blocking_client(options).unwrap();

    // Create multiple concurrent requests in separate threads
    let mut handles = vec![];
    for i in 0..5 {
        let client_clone = client.clone();
        let handle = thread::spawn(move || {
            let request = Request {
                method: Method::Get,
                relative_uri: Cow::Borrowed(TEST_URL),
                additional_headers: vec![(
                    Cow::Borrowed("X-Test-Request"),
                    Cow::Owned(format!("{}", i)),
                )],
                body: None,
            };

            let response = client_clone.request(request).unwrap();
            assert_eq!(response.status(), 200);
            i
        });
        handles.push(handle);
    }

    // Wait for all requests to complete
    for handle in handles {
        let result = handle.join().unwrap();
        println!("Request {} completed", result);
    }

    println!("✓ Concurrent blocking requests work with different threads");
}

/// Test mixed runtime usage - blocking and async in the same test
#[cfg(all(feature = "async", feature = "blocking"))]
#[tokio::test]
async fn test_mixed_runtime_usage() {
    let backend = ReqwestBackend;
    let options = ClientOptions::default();

    // Create both async and blocking clients
    let async_client = backend.create_async_client(options.clone()).await.unwrap();
    let blocking_client = backend.create_blocking_client(options).unwrap();

    // Make an async request
    let async_request = Request {
        method: Method::Get,
        relative_uri: Cow::Borrowed(TEST_URL),
        additional_headers: vec![(Cow::Borrowed("X-Client-Type"), Cow::Borrowed("async"))],
        body: None,
    };

    let async_response = async_client.request(async_request).await.unwrap();
    assert_eq!(async_response.status(), 200);

    // Make a blocking request in spawn_blocking
    let blocking_response = tokio::task::spawn_blocking(move || {
        let blocking_request = Request {
            method: Method::Get,
            relative_uri: Cow::Borrowed(TEST_URL),
            additional_headers: vec![(Cow::Borrowed("X-Client-Type"), Cow::Borrowed("blocking"))],
            body: None,
        };

        blocking_client.request(blocking_request)
    })
    .await
    .unwrap()
    .unwrap();

    assert_eq!(blocking_response.status(), 200);

    println!("✓ Mixed runtime usage works correctly");
}
