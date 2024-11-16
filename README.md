# Nyquest

[![crates.io](https://img.shields.io/crates/v/nyquest.svg)](https://crates.io/crates/nyquest)
[![Released API docs](https://docs.rs/nyquest/badge.svg)](https://docs.rs/nyquest)
[![MIT licensed](https://img.shields.io/badge/license-MIT-blue.svg)](./LICENSE)
[![CI](https://github.com/bdbai/nyquest/actions/workflows/run-tests.yml/badge.svg)](https://github.com/bdbai/nyquest/actions/workflows/run-tests.yml)

A truly native HTTP client library for Rust.

## Why Nyquest?

You should ask the other way around: why shipping an entire HTTP stack with your application when you know your users already have WinHTTP/NSURLSession/libcurl/whatever HTTP client library available on their system?

Similar to the [RustCrypto](https://github.com/RustCrypto) ecosystem, Nyquest aims to provide only an abstraction over the HTTP client operations. The actual functionality is implemented by Nyquest backends that talk to either platform APIs or third-party libraries. Even though Nyquest API interface has an async variant, it is not tied to any specific async runtime. Instead, the backends will as much as possible utilize the async mechanism or event loop provided by the system, or manage an event loop for the application. This way, end application developers can easily choose and switch among HTTP client implementations without thinking too much about the details. Library authors can also consume HTTP endpoints without worrying about which async runtime the end application uses.

By using platform native APIs, users will automatically benefit from system-managed security updates, functionality improvements, cache management, global proxy settings etc. that are tightly integrated with the operating system. [^1]

As an end application developer, consider Nyquest if you:

- want to behave like a good citizen on the user's system,
- want to honor the user's system settings for HTTP client, such as proxy,
- do not use an async runtime at all, or
- do not want to pull in the whole hyper or reqwest stack hence to reduce the binary size.

As a library author, consider Nyquest if you:

- want to provide a flexible way for users to choose their HTTP client implementation,
- do not want to assume the async runtime of the end application, or
- do not want to bring maintenance burden of depending on the whole hyper or reqwest stack to your users.

Meanwhile, you might not need Nyquest if you:

- want to minimize the overhead introduced by abstraction or interop with external libraries,
- want to keep every single byte of HTTP requests sent over the wire under your control,
- already have reqwest in your dependency tree, or
- are already maintaining bindings to various HTTP client libraries.

[^1]: Subject to the backend's capability.

## Roadmap

Nyquest is still at POC stage. We want to keep Nyquest itself as a greatest common divisor for all backends, therefore the API surface is subject to change along with the development of backends.

The following items are planned in MVP:

- [x] Nyquest blocking API (WIP)
- [x] Nyquest async API (WIP)
- [ ] URL manipulation utilities
- [x] Backend: WinRT HttpClient (WIP)
    - [x] Blocking (WIP)
    - [x] Async (WIP)
- [x] Backend: libcurl (WIP)
    - [x] Blocking (WIP)
    - [ ] Async (WIP)
- [x] Test framework for backends (WIP)
- [x] Presets (WIP)
- [ ] Documentation

Future work may include:

- [ ] Middleware infrastructure
- [ ] Telemetry
- [ ] Backend: NSURLSession (help wanted!)
- [ ] Backend: Mock
- [ ] Backend: WASM fetch
- [ ] Backend: WinHTTP
- [ ] Backend: libsoup3
- [ ] Backend: reqwest (yes, we can do that)
- [ ] Backend: QNetworkAccessManager
- [ ] Explore alternative options on Android other than libcurl

## License

Licensed under Apache License, Version 2.0 or MIT license, at your option.
