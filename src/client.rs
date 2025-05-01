//! Contains types for building a client.
//!

mod builder;
mod error;

pub use builder::ClientBuilder;
pub use error::{BuildClientError, BuildClientResult};
