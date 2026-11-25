//! WinRT progress tracking helpers.
//!
//! WinRT has a difference from libcurl and NSURLSession:
//!
//! - `BytesSent` includes request headers.
//! - `BytesReceived` includes response headers.
//! - `TotalBytesToSend` represents data bytes to send.
//! - `TotalBytesToReceive` represents data bytes to receive.
//!
//! Therefore, `BytesSent` and `BytesReceived` cannot be directly mapped to
//! Nyquest body byte counters. [`WinrtProgressTracker`] uses
//! `HttpProgressStage` to track the transition between headers and content.
//!
//! # Precision note
//!
//! Capturing the offset at the first `SendingContent` or `ReceivingContent`
//! notification is a practical approximation. The WinRT documentation does not
//! guarantee that the first notification for a content stage occurs before any
//! content bytes have been transferred. If that notification already includes
//! content bytes, those bytes will be included in the captured offset.

use std::sync::Arc;

use nyquest_interface::{Progress, ProgressReporter};

/// Mirrors `windows::Web::Http::HttpProgressStage` for testable pure logic.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ProgressStage {
    #[default]
    Created,
    WaitingForResponse,
    SendingHeaders,
    SendingContent,
    /// Kept for API completeness; WinRT has no such stage.
    #[allow(dead_code)]
    WaitingForContent,
    ReceivingHeaders,
    ReceivingContent,
    /// Kept for API completeness; WinRT has no such stage.
    #[allow(dead_code)]
    Completed,
}

/// Tracks WinRT header offsets and retry epochs.
///
/// See the module documentation for the design rationale.
pub struct WinrtProgressTracker {
    reporter: Arc<dyn ProgressReporter>,
    upload_header_offset: u64,
    download_header_offset: u64,
    upload_content_started: bool,
    download_content_started: bool,
    previous_retries: u32,
}

impl WinrtProgressTracker {
    /// Creates a new tracker that reports through `reporter`.
    pub fn new(reporter: Arc<dyn ProgressReporter>) -> Self {
        Self {
            reporter,
            upload_header_offset: 0,
            download_header_offset: 0,
            upload_content_started: false,
            download_content_started: false,
            previous_retries: 0,
        }
    }

    fn normalize_total(total: u64) -> Option<u64> {
        if total == 0 {
            None
        } else {
            Some(total)
        }
    }

    /// Handles a progress notification and reports a complete snapshot.
    ///
    /// `retries` corresponds to `HttpProgress.Retries`. When it changes, the
    /// upload and download header offsets and content-started flags are reset
    /// to start a new counting epoch.
    pub fn update(
        &mut self,
        stage: ProgressStage,
        bytes_sent: u64,
        bytes_received: u64,
        total_bytes_to_send: u64,
        total_bytes_to_receive: u64,
        retries: u32,
    ) -> Progress {
        if retries != self.previous_retries {
            self.previous_retries = retries;
            self.upload_header_offset = 0;
            self.download_header_offset = 0;
            self.upload_content_started = false;
            self.download_content_started = false;
        }
        match stage {
            ProgressStage::SendingHeaders => {
                self.upload_header_offset = 0;
                self.upload_content_started = false;
            }
            ProgressStage::ReceivingHeaders => {
                self.download_header_offset = 0;
                self.download_content_started = false;
            }
            ProgressStage::SendingContent if !self.upload_content_started => {
                self.upload_header_offset = bytes_sent;
                self.upload_content_started = true;
            }
            ProgressStage::ReceivingContent if !self.download_content_started => {
                self.download_header_offset = bytes_received;
                self.download_content_started = true;
            }
            _ => {}
        }
        let uploaded = bytes_sent.saturating_sub(self.upload_header_offset);
        let downloaded = bytes_received.saturating_sub(self.download_header_offset);
        let progress = Progress {
            uploaded,
            upload_total: Self::normalize_total(total_bytes_to_send),
            downloaded,
            download_total: Self::normalize_total(total_bytes_to_receive),
        };
        self.reporter.report(progress);
        progress
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    struct Collect {
        inner: Mutex<Vec<Progress>>,
    }

    impl ProgressReporter for Collect {
        fn report(&self, progress: Progress) {
            self.inner.lock().unwrap().push(progress);
        }
    }

    fn tracker() -> (WinrtProgressTracker, Arc<Collect>) {
        let collect = Arc::new(Collect {
            inner: Mutex::new(vec![]),
        });
        let tracker = WinrtProgressTracker::new(collect.clone());
        (tracker, collect)
    }

    #[test]
    fn header_offset_calculation() {
        let (mut tracker, _) = tracker();
        // Headers phase carries header bytes.
        let p = tracker.update(ProgressStage::SendingHeaders, 200, 0, 100, 0, 0);
        assert_eq!(p.uploaded, 200);
        // First content notification captures the offset.
        let p = tracker.update(ProgressStage::SendingContent, 200, 0, 100, 0, 0);
        assert_eq!(p.uploaded, 0);
        let p = tracker.update(ProgressStage::SendingContent, 250, 0, 100, 0, 0);
        assert_eq!(p.uploaded, 50);
        assert_eq!(p.upload_total, Some(100));
    }

    #[test]
    fn download_header_offset() {
        let (mut tracker, _) = tracker();
        let p = tracker.update(ProgressStage::ReceivingHeaders, 0, 150, 0, 500, 0);
        assert_eq!(p.downloaded, 150);
        let p = tracker.update(ProgressStage::ReceivingContent, 0, 150, 0, 500, 0);
        assert_eq!(p.downloaded, 0);
        let p = tracker.update(ProgressStage::ReceivingContent, 0, 300, 0, 500, 0);
        assert_eq!(p.downloaded, 150);
        assert_eq!(p.download_total, Some(500));
    }

    #[test]
    fn retry_resets_offsets() {
        let (mut tracker, _) = tracker();
        tracker.update(ProgressStage::SendingContent, 200, 0, 100, 0, 0);
        tracker.update(ProgressStage::SendingContent, 300, 0, 100, 0, 0);
        // Retry epoch restarts counting.
        let p = tracker.update(ProgressStage::SendingHeaders, 10, 0, 100, 0, 1);
        assert_eq!(p.uploaded, 10);
        let p = tracker.update(ProgressStage::SendingContent, 10, 0, 100, 0, 1);
        assert_eq!(p.uploaded, 0);
    }

    #[test]
    fn counter_regression_clamped() {
        let (mut tracker, _) = tracker();
        tracker.update(ProgressStage::SendingContent, 200, 0, 100, 0, 0);
        // Regression without retry change is clamped to zero, not forced monotonic.
        let p = tracker.update(ProgressStage::SendingContent, 150, 0, 100, 0, 0);
        assert_eq!(p.uploaded, 0);
    }

    #[test]
    fn unknown_totals() {
        let (mut tracker, _) = tracker();
        let p = tracker.update(ProgressStage::SendingContent, 0, 0, 0, 0, 0);
        assert_eq!(p.upload_total, None);
        assert_eq!(p.download_total, None);
    }

    #[test]
    fn no_upload_content() {
        let (mut tracker, _) = tracker();
        // No upload body: only receiving side progresses.
        tracker.update(ProgressStage::ReceivingHeaders, 0, 50, 0, 0, 0);
        let p = tracker.update(ProgressStage::ReceivingContent, 0, 50, 0, 500, 0);
        assert_eq!(p.uploaded, 0);
        assert_eq!(p.upload_total, None);
        // First content notification captures the header offset.
        assert_eq!(p.downloaded, 0);
        let p = tracker.update(ProgressStage::ReceivingContent, 0, 150, 0, 500, 0);
        assert_eq!(p.uploaded, 0);
        assert_eq!(p.upload_total, None);
        assert_eq!(p.downloaded, 100);
        assert_eq!(p.download_total, Some(500));
    }

    #[test]
    fn stage_transitions_reset() {
        let (mut tracker, _) = tracker();
        tracker.update(ProgressStage::SendingContent, 200, 0, 10, 0, 0);
        // Returning to headers resets the upload offset.
        let p = tracker.update(ProgressStage::SendingHeaders, 50, 0, 10, 0, 0);
        assert_eq!(p.uploaded, 50);
    }

    #[test]
    fn all_stages_yield_complete_snapshots() {
        use ProgressStage::*;
        let (mut tracker, collected) = tracker();
        for stage in [
            Created,
            WaitingForResponse,
            SendingHeaders,
            SendingContent,
            WaitingForContent,
            ReceivingHeaders,
            ReceivingContent,
            Completed,
        ] {
            tracker.update(stage, 10, 20, 100, 200, 0);
        }
        // Every notification, including unmapped stages, yields a report.
        assert_eq!(collected.inner.lock().unwrap().len(), 8);
    }
}
