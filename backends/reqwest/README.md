<!-- cargo-rdme start -->

# nyquest-backend-reqwest

reqwest backend for nyquest HTTP client library

This backend provides a reqwest-based implementation of the nyquest HTTP client interface, supporting both async and blocking operations.

TODO: why using nyquest-backend-reqwest over reqwest directly?
TODO: async runtime requirements

## Features

- **async**: Enable async interface support using reqwest's async client
- **blocking**: Enable blocking interface support using reqwest's blocking client on a background thread
- **multipart**: Enable multipart form support

## Usage

```rust
use nyquest_backend_reqwest::register;

// Register the reqwest backend as the default
register();

// Now you can use nyquest with the reqwest backend
// (This example requires the nyquest crate to be in scope)
// let response = nyquest::get("https://httpbin.org/get").await?;
```

<!-- cargo-rdme end -->