<div class="rustdoc-hidden">

# nyquest-backend-curl

</div>

libcurl backend for [`nyquest`].

**Note**: Requires libcurl **7.68.0** or later.

## Features

- `ssl`: Enable SSL support. This is enabled by default.
- `blocking`
- `async`
- `multipart`
- `charset-defaults`: Enable encoding conversion via [`iconv-native`] with its default features
  enabled.
- `charset`: Enable encoding conversion via [`iconv-native`] without activating any of its default
  features. Refer to the documentation of [`iconv-native`] for its features.

[`nyquest`]: https://docs.rs/nyquest
[`iconv-native`]: https://crates.io/crates/iconv-native
