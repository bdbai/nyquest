//! Blocking HTTP client implementation for WinHTTP backend.

mod client;
mod response;
#[cfg(feature = "ws")]
mod websocket;
