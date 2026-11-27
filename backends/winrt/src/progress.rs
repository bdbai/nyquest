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

use std::sync::{Arc, Mutex};

use nyquest_interface::{
    Error as NyquestError, Progress, ProgressReporter, Result as NyquestResult,
};
use windows::Web::Http::{HttpProgress, HttpProgressStage, HttpResponseMessage};
use windows_core::RuntimeType;
use windows_future::{AsyncOperationProgressHandler, IAsyncOperationWithProgress};

use crate::error::IntoNyquestResult;

/// Mirrors `windows::Web::Http::HttpProgressStage` for testable pure logic.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ProgressStage {
    #[default]
    Created,
    WaitingForResponse,
    SendingHeaders,
    SendingContent,
    ReceivingHeaders,
    ReceivingContent,
}

/// Tracks WinRT header offsets and retry epochs.
///
/// See the module documentation for the design rationale.
pub struct WinrtProgressTracker {
    reporter: Arc<dyn ProgressReporter>,
    upload_header_offset: Option<u64>,
    download_header_offset: Option<u64>,
    previous_retries: u32,
}

impl WinrtProgressTracker {
    /// Creates a new tracker that reports through `reporter`.
    pub fn new(reporter: Arc<dyn ProgressReporter>) -> Self {
        Self {
            reporter,
            upload_header_offset: None,
            download_header_offset: None,
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
    /// upload and download header offsets are reset to start a new counting
    /// epoch.
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
            self.upload_header_offset = None;
            self.download_header_offset = None;
        }
        match stage {
            ProgressStage::SendingHeaders => {
                self.upload_header_offset = None;
            }
            ProgressStage::ReceivingHeaders => {
                self.download_header_offset = None;
            }
            ProgressStage::SendingContent if self.upload_header_offset.is_none() => {
                self.upload_header_offset = Some(bytes_sent);
            }
            ProgressStage::ReceivingContent if self.download_header_offset.is_none() => {
                self.download_header_offset = Some(bytes_received);
            }
            _ => {}
        }
        let uploaded = bytes_sent.saturating_sub(self.upload_header_offset.unwrap_or(0));
        let downloaded = bytes_received.saturating_sub(self.download_header_offset.unwrap_or(0));
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

/// Shared progress state for a WinRT transfer.
///
/// Upload progress comes from the send operation's `HttpProgress`
/// notifications (mapped through [`WinrtProgressTracker`], which excludes
/// header bytes via stage-based offsets and resets counting epochs on
/// retries). Download progress during body consumption is counted from the
/// bytes actually yielded to the user, primed so the header offset captured
/// from the send operation applies consistently.
///
/// All callbacks only report through [`ProgressReporter`]; backend threads
/// never execute user code.
pub(crate) struct WinrtProgressSink {
    state: Mutex<WinrtProgressSinkState>,
}

struct WinrtProgressSinkState {
    tracker: WinrtProgressTracker,
    last_sent: u64,
    last_send_total: Option<u64>,
    last_retries: u32,
    body_downloaded: u64,
    body_total: Option<u64>,
    body_primed: bool,
}

impl WinrtProgressSink {
    pub(crate) fn new(reporter: Arc<dyn ProgressReporter>, upload_total: Option<u64>) -> Arc<Self> {
        let tracker = WinrtProgressTracker::new(reporter);
        let sink = Arc::new(Self {
            state: Mutex::new(WinrtProgressSinkState {
                tracker,
                last_sent: 0,
                last_send_total: upload_total,
                last_retries: 0,
                body_downloaded: 0,
                body_total: None,
                body_primed: false,
            }),
        });
        // Emit an initial snapshot so consumers observe the known totals
        // even before the first native notification arrives.
        sink.with_state(|state| {
            state.tracker.update(
                ProgressStage::Created,
                0,
                0,
                upload_total.unwrap_or(0),
                0,
                0,
            );
        });
        sink
    }

    fn with_state<R>(&self, f: impl FnOnce(&mut WinrtProgressSinkState) -> R) -> R {
        f(&mut self.state.lock().unwrap_or_else(|e| e.into_inner()))
    }

    /// Records the known download total (e.g. from `Content-Length`).
    pub(crate) fn set_body_total(&self, total: Option<u64>) {
        self.with_state(|state| {
            state.body_total = total;
        });
    }

    /// Handles a native `HttpProgress` notification from the send operation.
    pub(crate) fn on_http_progress(&self, progress: &HttpProgress) {
        let stage = match progress.Stage {
            HttpProgressStage::SendingHeaders => Some(ProgressStage::SendingHeaders),
            HttpProgressStage::SendingContent => Some(ProgressStage::SendingContent),
            HttpProgressStage::WaitingForResponse => Some(ProgressStage::WaitingForResponse),
            HttpProgressStage::ReceivingHeaders => Some(ProgressStage::ReceivingHeaders),
            HttpProgressStage::ReceivingContent => Some(ProgressStage::ReceivingContent),
            _ => None,
        };
        let total_send = progress
            .TotalBytesToSend
            .as_ref()
            .and_then(|total| total.Value().ok());
        let total_recv = progress
            .TotalBytesToReceive
            .as_ref()
            .and_then(|total| total.Value().ok());
        self.with_state(|state| {
            state.last_sent = progress.BytesSent;
            state.last_send_total = total_send;
            state.last_retries = progress.Retries;
            if let Some(stage) = stage {
                state.tracker.update(
                    stage,
                    progress.BytesSent,
                    progress.BytesReceived,
                    total_send.unwrap_or(0),
                    total_recv.unwrap_or(0),
                    progress.Retries,
                );
            }
        });
    }

    /// Ensures the body-download offset is primed before body bytes are
    /// counted, so header bytes reported during the send operation do not
    /// leak into body byte counters.
    fn prime_body_download(state: &mut WinrtProgressSinkState) {
        if !state.body_primed {
            state.body_primed = true;
            state.tracker.update(
                ProgressStage::ReceivingContent,
                state.last_sent,
                0,
                state.last_send_total.unwrap_or(0),
                state.body_total.unwrap_or(0),
                state.last_retries,
            );
        }
    }

    /// Reports `cumulative` body bytes received (e.g. from a read operation's
    /// progress notification, treated as a cumulative count).
    pub(crate) fn on_body_progress(&self, cumulative: u64) {
        self.with_state(|state| {
            Self::prime_body_download(state);
            state.body_downloaded = state.body_downloaded.max(cumulative);
            let downloaded = state.body_downloaded;
            let total = state.body_total;
            let (sent, send_total, retries) =
                (state.last_sent, state.last_send_total, state.last_retries);
            state.tracker.update(
                ProgressStage::ReceivingContent,
                sent,
                downloaded,
                send_total.unwrap_or(0),
                total.unwrap_or(0),
                retries,
            );
        });
    }

    /// Adds newly consumed body bytes (e.g. from a streaming read).
    #[cfg(any(
        all(feature = "async-progress", feature = "async-stream"),
        all(feature = "blocking-progress", feature = "blocking-stream")
    ))]
    pub(crate) fn on_body_chunk(&self, len: u64) {
        if len == 0 {
            return;
        }
        self.with_state(|state| {
            Self::prime_body_download(state);
            state.body_downloaded = state.body_downloaded.saturating_add(len);
            let downloaded = state.body_downloaded;
            let total = state.body_total;
            let (sent, send_total, retries) =
                (state.last_sent, state.last_send_total, state.last_retries);
            state.tracker.update(
                ProgressStage::ReceivingContent,
                sent,
                downloaded,
                send_total.unwrap_or(0),
                total.unwrap_or(0),
                retries,
            );
        });
    }

    /// Reports the exact final body size (e.g. after buffering the body).
    pub(crate) fn on_body_complete(&self, total: u64) {
        self.with_state(|state| {
            Self::prime_body_download(state);
            state.body_downloaded = total;
            let downloaded = state.body_downloaded;
            let body_total = state.body_total;
            let (sent, send_total, retries) =
                (state.last_sent, state.last_send_total, state.last_retries);
            state.tracker.update(
                ProgressStage::ReceivingContent,
                sent,
                downloaded,
                send_total.unwrap_or(0),
                body_total.unwrap_or(0),
                retries,
            );
        });
    }
}

/// Hooks a native progress handler onto a body-read operation.
///
/// The handler enforces `max_response_buffer_size` (the same limit otherwise
/// enforced without progress reporting) and, when `sink` is present, reports
/// cumulative download progress through it.
pub(crate) fn hook_read_task_progress<TResult: RuntimeType>(
    task: &IAsyncOperationWithProgress<TResult, u64>,
    sink: Option<&Arc<WinrtProgressSink>>,
    max_response_buffer_size: Option<u64>,
) -> NyquestResult<ProgressReadGuard> {
    use std::sync::atomic::{AtomicBool, Ordering};

    let exceeded = Arc::new(AtomicBool::new(false));
    let exceeded_hook = exceeded.clone();
    let sink_hook = sink.cloned();
    task.SetProgress(&AsyncOperationProgressHandler::new(
        move |op, progress: windows_core::Ref<'_, u64>| {
            if let Some(max) = max_response_buffer_size {
                if let Some(received) = progress.as_ref() {
                    if *received > max {
                        exceeded_hook.store(true, Ordering::SeqCst);
                        op.as_ref().map(|t| t.Cancel().ok());
                    }
                }
            }
            if let (Some(sink), Some(received)) = (sink_hook.as_ref(), progress.as_ref()) {
                sink.on_body_progress(*received);
            }
            Ok(())
        },
    ))
    .into_nyquest_result()?;
    Ok(ProgressReadGuard {
        exceeded,
        max_response_buffer_size,
        sink: sink.cloned(),
    })
}

/// Size guard returned by [`hook_read_task_progress`], mirroring the
/// response-size limiter used for transfers without progress reporting.
#[must_use = "call assert_size to enforce response size limit"]
pub(crate) struct ProgressReadGuard {
    exceeded: Arc<std::sync::atomic::AtomicBool>,
    max_response_buffer_size: Option<u64>,
    sink: Option<Arc<WinrtProgressSink>>,
}

impl ProgressReadGuard {
    pub(crate) fn assert_size<T: AsRef<[u8]>>(self, res: NyquestResult<T>) -> NyquestResult<T> {
        use std::sync::atomic::Ordering;

        if self.exceeded.load(Ordering::SeqCst) {
            return Err(NyquestError::ResponseTooLarge);
        }
        let res = res?;
        if let Some(max) = self.max_response_buffer_size {
            if res.as_ref().len() > max as usize {
                return Err(NyquestError::ResponseTooLarge);
            }
        }
        // Report the exact buffered size to reconcile any imprecision in the
        // intermediate progress notifications.
        if let Some(sink) = &self.sink {
            sink.on_body_complete(res.as_ref().len() as u64);
        }
        Ok(res)
    }
}

/// Hooks [`WinrtProgressSink::on_http_progress`] onto a send operation.
pub(crate) fn hook_send_progress(
    task: &IAsyncOperationWithProgress<HttpResponseMessage, HttpProgress>,
    sink: &Arc<WinrtProgressSink>,
) -> NyquestResult<()> {
    let sink = sink.clone();
    task.SetProgress(&AsyncOperationProgressHandler::new(
        move |_, progress: windows_core::Ref<'_, HttpProgress>| {
            if let Some(p) = progress.as_ref() {
                sink.on_http_progress(p);
            }
            Ok(())
        },
    ))
    .into_nyquest_result()?;
    Ok(())
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
            ReceivingHeaders,
            ReceivingContent,
        ] {
            tracker.update(stage, 10, 20, 100, 200, 0);
        }
        // Every notification, including unmapped stages, yields a report.
        assert_eq!(collected.inner.lock().unwrap().len(), 6);
    }
}
