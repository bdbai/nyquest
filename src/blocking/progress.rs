//! Blocking progress reporting for the `nyquest` facade.
//!
//! Backends produce progress via [`ProgressReporter`]. This module implements
//! delivery to users: a callback-backed reporter for blocking clients.

use nyquest_interface::{Progress, ProgressReporter};

/// Callback-backed reporter for blocking progress reporting.
///
/// The callback is invoked synchronously from [`ProgressReporter::report`],
/// which for blocking transfers runs on the calling thread.
pub(crate) struct CallbackProgressReporter<F> {
    callback: F,
}

impl<F> CallbackProgressReporter<F>
where
    F: Fn(Progress) + Send + Sync,
{
    pub(crate) fn new(callback: F) -> Self {
        Self { callback }
    }
}

impl<F> ProgressReporter for CallbackProgressReporter<F>
where
    F: Fn(Progress) + Send + Sync,
{
    fn report(&self, progress: Progress) {
        (self.callback)(progress);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blocking_reporter_calls_callback() {
        let seen = std::sync::Mutex::new(Vec::new());
        let reporter = CallbackProgressReporter::new(|p: Progress| {
            seen.lock().unwrap().push(p);
        });
        let snapshot = Progress {
            uploaded: 1024,
            upload_total: Some(4096),
            downloaded: 2048,
            download_total: Some(8192),
        };
        reporter.report(snapshot);
        assert_eq!(seen.lock().unwrap().as_slice(), &[snapshot]);
    }
}
