<div class="rustdoc-hidden">

# nyquest-backend-curl

</div>

libcurl backend for [`nyquest`].

**Note**: Requires libcurl **7.68.0** or later.

## Features

- `ssl`: Enable SSL support. This is enabled by default.
- `blocking`
- `blocking-stream`
- `async`
- `async-stream`
- `multipart`
- `charset-defaults`: Enable encoding conversion via [`iconv-native`] with its default features
  enabled.
- `charset`: Enable encoding conversion via [`iconv-native`] without activating any of its default
  features. Refer to the documentation of [`iconv-native`] for its features.

## Client-specific Customization

A client can set arbitrary `CURLOPT`s that Nyquest has no corresponding option for. Build a customized backend and hand it to `ClientBuilder::custom_backend`:

```rust
use nyquest_backend_curl::{curl_sys, CurlBackend, CurlHandle, HandleCustomizer};
use nyquest_interface::Request;

// There is no need to call `nyquest_backend_curl::register()` or `init()`: this client uses the
// backend below instead of the global default.
struct Customizer;

impl HandleCustomizer for Customizer {
    fn customize_handle(
        &self,
        mut handle: CurlHandle<'_>,
    ) -> Result<(), nyquest_backend_curl::CurlCodeContext> {
        unsafe {
            handle.setopt_long(curl_sys::CURLOPT_TCP_KEEPALIVE, 1)?;
            handle.setopt_ptr_raw(curl_sys::CURLOPT_REFERER, c"custom-referrer".as_ptr())?;
        }
        Ok(())
    }
}

let backend = CurlBackend::customize(Customizer);

// Then use the backend to build a client:
// let client = nyquest::ClientBuilder::default()
//     .custom_backend(&backend)
//     .build_blocking()?;
```

The customizer is applied to the easy handle of every request, after the built-in
`nyquest_interface::client::ClientOptions` have been set, so it has the final say over the
resulting request. It exposes the handle as a [`CurlHandle`] whose setters mirror `curl_easy_setopt`
— [`CurlHandle::setopt_long`], [`CurlHandle::setopt_off_t`], [`CurlHandle::setopt_ptr_owned`], [`CurlHandle::setopt_ptr_raw`], [`CurlHandle::setopt_slist`], [`CurlHandle::setopt_blob`] — plus raw access to the `*mut CURL` via `as_raw` for anything else. Calling any of those setters is `unsafe`, because the caller is the only one who knows which value the option they name takes; see [`CurlBackend::customize`] and [Why the setters are `unsafe`](#why-the-setters-are-unsafe).
The constants an option is named by come from the [`curl_sys`], which this crate re-exports from [`curl-sys`](https://crates.io/crates/curl-sys) crate.

### Options reserved by Nyquest

The following options are managed internally by this backend and **must not be set by a
customizer**. Setting any of them can interfere with Nyquest's internal processing and may cause
undefined behavior:

`CURLOPT_NOSIGNAL`, `CURLOPT_NOPROXY`, `CURLOPT_PROXY`, `CURLOPT_HTTPPROXYTUNNEL`,
`CURLOPT_SUPPRESS_CONNECT_HEADERS`, `CURLOPT_USERAGENT`, `CURLOPT_COOKIEFILE`, `CURLOPT_TIMEOUT`,
`CURLOPT_TIMEOUT_MS`, `CURLOPT_SSL_VERIFYPEER`, `CURLOPT_SSL_VERIFYHOST`,
`CURLOPT_FOLLOWLOCATION`, `CURLOPT_URL`, `CURLOPT_HTTPGET`, `CURLOPT_POST`, `CURLOPT_UPLOAD`,
`CURLOPT_NOBODY`, `CURLOPT_CUSTOMREQUEST`, `CURLOPT_INFILESIZE_LARGE`, `CURLOPT_POSTFIELDS`,
`CURLOPT_POSTFIELDSIZE_LARGE`, `CURLOPT_COPYPOSTFIELDS`, `CURLOPT_ACCEPT_ENCODING`,
`CURLOPT_WRITEDATA`, `CURLOPT_WRITEFUNCTION`, `CURLOPT_READDATA`, `CURLOPT_READFUNCTION`,
`CURLOPT_HEADERDATA`, `CURLOPT_HEADERFUNCTION`, `CURLOPT_SEEKDATA`, `CURLOPT_SEEKFUNCTION`,
`CURLOPT_HTTPHEADER`, `CURLOPT_MIMEPOST`, `CURLOPT_PRIVATE`, `CURLOPT_SHARE`, and
`CURLOPT_ERRORBUFFER`.

### Ownership

The customizer is shared by every handle of the client, so it cannot simply own mutable data that an
option has to keep alive: libcurl copies most values, but a `struct curl_slist`, a
`struct curl_blob` and an application data pointer are read again during the transfer, long after
the customizer has returned. `CurlHandle` therefore provides owning setters that move such data into the
easy handle itself, where it lives until the same option is set again, until the handle is reset,
or until the handle is dropped:

```rust
use nyquest_backend_curl::{curl_sys, CurlBackend, CurlHandle, CurlStringList, HandleCustomizer};

struct ExtraHeaders {
    headers: CurlStringList,
}

impl HandleCustomizer for ExtraHeaders {
    fn customize_handle(
        &self,
        mut handle: CurlHandle<'_>,
    ) -> Result<(), nyquest_backend_curl::CurlCodeContext> {
        // Safety: the option takes a `struct curl_slist`, which the handle now owns and frees.
        unsafe { handle.setopt_slist(curl_sys::CURLOPT_QUOTE, self.headers.clone()) }
    }
}

let mut extra_headers = CurlStringList::default();
extra_headers.append("x-tenant: acme");

let backend = CurlBackend::customize(ExtraHeaders { headers: extra_headers });
```

Which setter an option needs follows libcurl's own conventions:

| Category | Examples | Convention | Setter |
| --- | --- | --- | --- |
| String | `CURLOPT_REFERER`, `CURLOPT_CAINFO` | Copied by libcurl since 7.17.0, except `CURLOPT_POSTFIELDS` which is handled internally and should not be touched | `setopt_ptr_raw` |
| List | `CURLOPT_QUOTE`, `CURLOPT_RESOLVE` | Never copied; the last list set wins; the handle frees it | `setopt_slist` |
| Blob | `CURLOPT_CAINFO_BLOB`, `CURLOPT_SSLCERT_BLOB` | Copied only with `CURL_BLOB_COPY`, which pre-7.77.0 silently ignores | `setopt_blob` |
| Data pointer | `CURLOPT_PROGRESSDATA`, `CURLOPT_SSL_CTX_DATA` | Opaque; passed to the matching callback | `setopt_ptr_owned`, `setopt_ptr_raw` |
| Function pointer | `CURLOPT_DEBUGFUNCTION`, `CURLOPT_SEEKFUNCTION` | Called for the duration of the transfer | `setopt_ptr_raw` |
| Other pointer | `CURLOPT_PRIVATE`, `CURLOPT_SSL_CTX_DATA` | Read through the pointer | `setopt_ptr_owned`, `setopt_ptr_raw` |

### Why the setters are `unsafe`

Every setter requires the caller to know which
value the option they name takes, because `curl_easy_setopt` is variadic and libcurl reads
whatever it is given as the kind of value its option expects:

- an option that takes a `long` or a `curl_off_t` reads a pointer **as a number**;
- an option that takes any kind of pointer **follows** it, as a C string, as a `struct curl_slist`,
  as a `struct curl_blob` or as callback data;
- a function pointer option **calls** it.

That is also why the option number cannot be checked for the caller. `curl.h` defines an option as
`type + number`, and every option that takes an object pointer shares a single type, so
`CURLOPT_REFERER`, `CURLOPT_CAINFO_BLOB` and `CURLOPT_DEBUGFUNCTION` are
indistinguishable from their numbers alone. The setters take care of everything that *can* be
decided — the lifetime of the data, which the handle owns, and the shape of the value, which the
setter fixes — and leave the meaning of the option to the caller.

[`nyquest`]: https://docs.rs/nyquest
[`iconv-native`]: https://crates.io/crates/iconv-native

## License

Licensed under Apache License, Version 2.0 or MIT license, at your option.

Part of the code in this crate is adapted from [`curl-rust`](https://github.com/alexcrichton/curl-rust) licensed under the [MIT license](https://github.com/alexcrichton/curl-rust/blob/main/LICENSE).
