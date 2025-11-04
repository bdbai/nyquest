# nyquest-preset

[![crates.io](https://img.shields.io/crates/v/nyquest-preset.svg)](https://crates.io/crates/nyquest-preset)
[![Released API docs](https://docs.rs/nyquest-preset/badge.svg)](https://docs.rs/nyquest-preset)
[![MIT licensed](https://img.shields.io/badge/license-MIT-blue.svg)](./LICENSE)

Nyquest preset configuration with up-to-date rich-featured backends.

`nyquest-preset` is the official, default backend provider of [`nyquest`] that integrates
[`nyquest-backend-winrt`], [`nyquest-backend-nsurlsession`] and [`nyquest-backend-curl`]
into a uniform interface. The only exposed APIs are the `register` function and the
`Backend` type of the underlying backend.

This crate is intended to be consumed by end application users. Since there can be only one
backend registered as the global default, library authors in general are not recommended to
declare this crate as a dependency. Libraries should use [`nyquest`] instead.

## Quick Start

Add the following at your program startup:

```rust
nyquest_backend::register();
```

Based on the target platform, a [`nyquest`] backend will be registered as the default. Refer to
the documentation of [`nyquest`] for usages.

## Platform Support

`nyquest-preset` uses `cfg` to select the appropriate backend for the target platform.

- `windows`: [`nyquest-backend-winrt`]
- `target_vendor = "apple"`: [`nyquest-backend-nsurlsession`]
- others: [`nyquest-backend-curl`]

## Features

- `async`: Enable async support for backends and [`nyquest`].
- `blocking`: Enable blocking support for backends and [`nyquest`].
- `multipart`: Enable multipart form support for backends and [`nyquest`].
- `auto-register`: Automatically register the backend before program startup using [`ctor`](https://docs.rs/ctor). Recommended to be used in tests and examples.

Refer to the backends' documentation for more optional features. For example, enable
`charset-defaults` for [`nyquest-backend-curl`] to perform encoding conversion automatically
when the response has an encoding other than UTF-8.

## License

See [`nyquest#License`](../../README.md#license).

[`nyquest`]: ../..
[`nyquest-backend-winrt`]: ../../backends/winrt
[`nyquest-backend-nsurlsession`]: ../../backends/nsurlsession
[`nyquest-backend-curl`]: ../../backends/curl
