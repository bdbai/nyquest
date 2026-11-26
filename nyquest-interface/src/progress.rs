//! Progress tracking definitions for nyquest HTTP clients.
//!
//! This module provides the common progress data model and reporting contract
//! shared by all backends. Backends produce progress information, `nyquest-interface`
//! defines the reporting contract, and the `nyquest` facade implements delivery.

use std::sync::Arc;

/// Cumulative snapshot of HTTP transfer progress.
///
/// Both upload and download counters are reported together, even if only one
/// direction changed since the previous report.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Progress {
    /// Number of bytes uploaded so far.
    pub uploaded: u64,

    /// Expected number of bytes to upload, if known.
    ///
    /// `Some(0)` means the total is known to be zero.
    /// `Some(n)` means the expected total is known.
    /// `None` means the total is unknown.
    pub upload_total: Option<u64>,

    /// Number of bytes downloaded so far.
    pub downloaded: u64,

    /// Expected number of bytes to download, if known.
    ///
    /// `Some(0)` means the total is known to be zero.
    /// `Some(n)` means the expected total is known.
    /// `None` means the total is unknown.
    pub download_total: Option<u64>,
}

/// Contract for reporting transfer progress from backends.
///
/// Backend implementations receive a reporter through an `Arc` and call
/// [`ProgressReporter::report`] with complete [`Progress`] snapshots.
///
/// The trait does not depend on Tokio or any other async runtime.
/// `report()` is synchronous, lightweight, and must not block network
/// transfer callbacks. It must not execute user code directly; the concrete
/// reporter implementation in the `nyquest` facade is responsible for
/// delivering progress to users without running user callbacks on backend
/// threads (for async) or with appropriate synchronization (for blocking).
pub trait ProgressReporter: Send + Sync {
    /// Reports a complete cumulative progress snapshot.
    fn report(&self, progress: Progress);
}

impl<F> ProgressReporter for F
where
    F: Fn(Progress) + Send + Sync,
{
    fn report(&self, progress: Progress) {
        (self)(progress);
    }
}

/// Convenience: an `Arc` wrapping any reporter is itself a reporter.
impl ProgressReporter for Arc<dyn ProgressReporter> {
    fn report(&self, progress: Progress) {
        (**self).report(progress);
    }
}
