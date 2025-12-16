<!-- cargo-rdme start -->

# nyquest-backend-reqwest

reqwest backend for nyquest HTTP client library

This backend provides a reqwest-based implementation of the nyquest HTTP client interface, supporting both async and blocking operations.

Additionally, this backend supports WebAssembly (WASM) targets using reqwest's WASM capabilities.

## Use Cases

It may seem unintuitive to use nyquest-backend-reqwest when you could directly use reqwest itself, or even nyquest with its default presets. However, this crate is particularly valuable when your application uses dependencies that require nyquest, but the default presets offer minimal benefits—for example, when your application already includes reqwest in its dependency tree.

Another benefit of this crate is handling async runtime requirements even in blocking scenarios. Reqwest has specific runtime constraints:

- Its async variant requires a tokio runtime
- Its blocking variant panics when used inside a tokio runtime

While mixing async and blocking code is generally discouraged, some situations make it unavoidable. In these cases, nyquest-backend-reqwest helps by isolating the reqwest client from your application's async runtime through internal runtime management. Additionally, nyquest-backend-reqwest supports non-tokio async runtimes like async-std.

## Features

- **async**: Enable async interface support using reqwest's async client
- **async-stream**: Enable async interface and streaming upload/download support
- **blocking**: Enable blocking interface support using reqwest's blocking client on a background thread
- **blocking-stream**: Enable blocking interface and streaming upload/download support using reqwest's blocking client on a background thread
- **multipart**: Enable multipart form support
- **charset**: Enable charset conversion support using the `encoding_rs` crate

### TLS features

- **default-tls** (enabled by default): Enable `reqwest`'s `default-tls` feature
- **rustls-tls-minimal**: Enable `reqwest`'s `rustls-tls-manual-roots-no-provider` feature
- **native-tls**: Enable `reqwest`'s `native-tls` feature

At least one TLS feature must be enabled for this crate to function. Since `reqwest` provides numerous features, we only expose the essential TLS-related ones rather than re-exporting them all. If you require finer control over `reqwest` features, we recommend adding `reqwest` as a direct dependency in your project.

## Usage

```rust
// Register the reqwest backend as the default
nyquest_backend_reqwest::register();

// Now you can use nyquest with the reqwest backend
// (This example requires the nyquest crate to be in scope)
// let response = nyquest::r#async::get("https://httpbin.org/get").await?;
```

<!-- cargo-rdme end -->
