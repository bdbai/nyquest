//! HTTP client configuration and building.
//!
//! This module provides types and options for configuring and building
//! HTTP clients in nyquest.

mod error;
mod options;

pub use error::{BuildClientError, BuildClientResult};
pub use options::{CachingBehavior, ClientOptions};
