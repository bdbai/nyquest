//! Raw WinHTTP handle wrappers.
//!
//! This module provides safe wrappers around WinHTTP handles (HINTERNET),
//! following the pattern from curl_ng for raw handle management.

mod connection;
mod request;
mod session;

pub(crate) use connection::ConnectionHandle;
pub(crate) use request::RequestHandle;
pub(crate) use session::SessionHandle;
