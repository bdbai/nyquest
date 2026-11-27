//! WinHTTP progress tracking helpers.
//!
//! Implements progress reporting on top of WinHTTP status notifications,
//! normalizing reported byte counts and totals to the four-field [`Progress`]
//! model. Callbacks only report through [`ProgressReporter`], without
//! executing user code.

use std::sync::Arc;

use nyquest_interface::{Progress, ProgressReporter};

/// Normalizes a WinHTTP byte total: zero means unknown.
pub fn normalize_total(total: u64) -> Option<u64> {
    if total == 0 {
        None
    } else {
        Some(total)
    }
}

/// Tracks upload and download byte counts for WinHTTP transfers.
pub struct WinHttpProgressTracker {
    reporter: Arc<dyn ProgressReporter>,
    state: std::sync::Mutex<Progress>,
}

impl WinHttpProgressTracker {
    /// Creates a new tracker reporting through `reporter`.
    pub fn new(
        reporter: Arc<dyn ProgressReporter>,
        upload_total: Option<u64>,
        download_total: Option<u64>,
    ) -> Arc<Self> {
        let tracker = Arc::new(Self {
            reporter,
            state: std::sync::Mutex::new(Progress {
                uploaded: 0,
                upload_total,
                downloaded: 0,
                download_total,
            }),
        });
        tracker.report_current();
        tracker
    }

    fn report_current(&self) {
        let snapshot = *self.state.lock().unwrap_or_else(|e| e.into_inner());
        self.reporter.report(snapshot);
    }

    /// Records uploaded bytes.
    pub fn add_uploaded(&self, n: u64) {
        if n == 0 {
            return;
        }
        {
            let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
            state.uploaded = state.uploaded.saturating_add(n);
        }
        self.report_current();
    }

    /// Records downloaded bytes.
    pub fn add_downloaded(&self, n: u64) {
        if n == 0 {
            return;
        }
        {
            let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
            state.downloaded = state.downloaded.saturating_add(n);
        }
        self.report_current();
    }

    /// Updates the expected download total (e.g. from `Content-Length`).
    pub fn set_download_total(&self, total: Option<u64>) {
        {
            let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
            state.download_total = total;
        }
        self.report_current();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    /// Builds a complete [`Progress`] snapshot.
    fn make_progress(
        uploaded: u64,
        upload_total: Option<u64>,
        downloaded: u64,
        download_total: Option<u64>,
    ) -> Progress {
        Progress {
            uploaded,
            upload_total,
            downloaded,
            download_total,
        }
    }

    struct Collect {
        inner: Mutex<Vec<Progress>>,
    }

    impl ProgressReporter for Collect {
        fn report(&self, progress: Progress) {
            self.inner.lock().unwrap().push(progress);
        }
    }

    #[test]
    fn complete_snapshots() {
        let collect = Arc::new(Collect {
            inner: Mutex::new(vec![]),
        });
        let tracker = WinHttpProgressTracker::new(collect.clone(), Some(100), Some(200));
        // The initial report is an empty snapshot with the known totals.
        assert_eq!(
            *collect.inner.lock().unwrap().last().unwrap(),
            make_progress(0, Some(100), 0, Some(200))
        );
        tracker.add_uploaded(10);
        tracker.add_downloaded(20);
        let reports = collect.inner.lock().unwrap();
        let last = reports.last().unwrap();
        assert_eq!(last.uploaded, 10);
        assert_eq!(last.downloaded, 20);
        assert_eq!(last.upload_total, Some(100));
        assert_eq!(last.download_total, Some(200));
        assert_eq!(*last, make_progress(10, Some(100), 20, Some(200)));
    }

    #[test]
    fn zero_total_is_unknown() {
        assert_eq!(normalize_total(0), None);
        assert_eq!(normalize_total(5), Some(5));
    }
}
