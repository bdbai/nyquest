//! Async progress reporting for the `nyquest` facade.
//!
//! Backends produce progress via [`ProgressReporter`]. This module implements
//! delivery to users: a channel-backed reporter with [`ProgressStream`] for
//! async clients.
//!
//! The async architecture preserves runtime independence:
//!
//! ```text
//! Backend thread
//!       |
//!       | ProgressReporter::report()
//!       v
//! nyquest concrete reporter
//!       |
//!       v
//! Thread-safe progress channel
//!       |
//!       v
//! ProgressStream
//!       |
//!       | User polls Stream
//!       v
//! User async task
//!       |
//!       v
//! User progress handling
//! ```
//!
//! User progress handling executes in the context in which the user polls the
//! stream. No runtime handle is required.

use std::sync::{Arc, Mutex};
use std::task::{Context, Poll, Waker};

use nyquest_interface::{Progress, ProgressReporter};

#[derive(Debug, Default)]
struct ChannelProgressReporterInner {
    latest: Progress,
    version: u64,
    closed: bool,
    waker: Option<Waker>,
}

/// Channel-backed reporter for async progress reporting.
///
/// Accepts reports from arbitrary backend threads, updates thread-safe shared
/// state, and wakes the [`ProgressStream`] consumer when new progress is
/// available. Intermediate notifications are coalesced: only the latest
/// snapshot is retained, rather than accumulating an unbounded queue.
///
/// Does not execute user code and does not depend on any async runtime.
#[derive(Debug)]
pub(crate) struct ChannelProgressReporter {
    inner: Mutex<ChannelProgressReporterInner>,
}

impl ProgressReporter for ChannelProgressReporter {
    fn report(&self, progress: Progress) {
        let waker = {
            let mut inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
            inner.latest = progress;
            inner.version = inner.version.wrapping_add(1);
            inner.waker.take()
        };
        if let Some(waker) = waker {
            waker.wake();
        }
    }
}

impl Drop for ChannelProgressReporter {
    fn drop(&mut self) {
        let waker = {
            let mut inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
            inner.closed = true;
            inner.waker.take()
        };
        if let Some(waker) = waker {
            waker.wake();
        }
    }
}

/// A stream of [`Progress`] snapshots.
///
/// Implements `futures_core::Stream<Item = Progress>`. The stream remains
/// usable after the request future completes, continuing to yield download
/// progress while the response body is consumed. It terminates (`None`) when
/// all reporters have been dropped (e.g. response dropped or body fully
/// consumed and backend released its reporter).
#[derive(Debug)]
pub struct ProgressStream {
    shared: Arc<ChannelProgressReporter>,
    seen_version: u64,
}

impl ProgressStream {
    pub(crate) fn new() -> Self {
        let shared = Arc::new(ChannelProgressReporter {
            inner: Mutex::new(ChannelProgressReporterInner::default()),
        });
        Self {
            shared,
            seen_version: 0,
        }
    }

    pub(crate) fn progress_reporter(&self) -> &Arc<ChannelProgressReporter> {
        &self.shared
    }
}

impl futures_core::Stream for ProgressStream {
    type Item = Progress;

    fn poll_next(self: std::pin::Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = self.get_mut();
        let mut inner = this.shared.inner.lock().unwrap_or_else(|e| e.into_inner());
        if inner.version != this.seen_version {
            this.seen_version = inner.version;
            let latest = inner.latest;
            Poll::Ready(Some(latest))
        } else if inner.closed || Arc::strong_count(&this.shared) == 1 {
            // `strong_count == 1` means only the stream itself retains the
            // reporter: all backend clones have been dropped. This covers the
            // case where `Drop` for the reporter has not run yet because the
            // stream still holds a strong reference.
            Poll::Ready(None)
        } else {
            inner.waker = Some(cx.waker().clone());
            Poll::Pending
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures_core::Stream;
    use std::task::{RawWaker, RawWakerVTable, Waker};

    fn noop_waker() -> Waker {
        fn clone(_: *const ()) -> RawWaker {
            RawWaker::new(std::ptr::null(), &VTABLE)
        }
        fn noop(_: *const ()) {}
        static VTABLE: RawWakerVTable = RawWakerVTable::new(clone, noop, noop, noop);
        unsafe { Waker::from_raw(RawWaker::new(std::ptr::null(), &VTABLE)) }
    }

    fn poll_next_once(stream: &mut ProgressStream) -> Poll<Option<Progress>> {
        let waker = noop_waker();
        let mut cx = Context::from_waker(&waker);
        std::pin::Pin::new(stream).poll_next(&mut cx)
    }

    #[test]
    fn async_stream_yields_reports() {
        let mut stream = ProgressStream::new();
        let reporter = stream.progress_reporter().clone();
        let p1 = Progress {
            uploaded: 10,
            upload_total: Some(100),
            downloaded: 0,
            download_total: None,
        };
        reporter.report(p1);
        match poll_next_once(&mut stream) {
            Poll::Ready(Some(got)) => assert_eq!(got, p1),
            _ => panic!("expected progress"),
        }
        let p2 = Progress {
            uploaded: 100,
            upload_total: Some(100),
            downloaded: 50,
            download_total: Some(200),
        };
        reporter.report(p2);
        match poll_next_once(&mut stream) {
            Poll::Ready(Some(got)) => assert_eq!(got, p2),
            _ => panic!("expected progress"),
        }
    }

    #[test]
    fn coalescing_retains_latest() {
        let mut stream = ProgressStream::new();
        let reporter = stream.progress_reporter().clone();
        for i in 1..=1000u64 {
            reporter.report(Progress {
                uploaded: i,
                upload_total: Some(1000),
                downloaded: 0,
                download_total: None,
            });
        }
        // Without intermediate polling, only the latest snapshot is retained.
        match poll_next_once(&mut stream) {
            Poll::Ready(Some(got)) => assert_eq!(got.uploaded, 1000),
            _ => panic!("expected coalesced progress"),
        }
        // No further queued events.
        match poll_next_once(&mut stream) {
            Poll::Pending => {}
            _ => panic!("expected pending after draining coalesced update"),
        }
    }

    #[test]
    fn stream_closes_when_reporter_dropped() {
        let mut stream = ProgressStream::new();
        let reporter = stream.progress_reporter().clone();
        reporter.report(Progress {
            uploaded: 1,
            upload_total: None,
            downloaded: 0,
            download_total: None,
        });
        match poll_next_once(&mut stream) {
            Poll::Ready(Some(_)) => {}
            _ => panic!("expected progress"),
        }
        drop(reporter);
        match poll_next_once(&mut stream) {
            Poll::Ready(None) => {}
            _ => panic!("expected stream termination"),
        }
    }

    #[test]
    fn stream_dropped_before_completion() {
        let stream = ProgressStream::new();
        let reporter = stream.progress_reporter().clone();
        drop(stream);
        // Reporting after the consumer is gone must not panic or block.
        for _ in 0..100 {
            reporter.report(Progress {
                uploaded: 1,
                upload_total: None,
                downloaded: 2,
                download_total: None,
            });
        }
    }

    #[test]
    fn large_numbers_of_notifications() {
        let mut stream = ProgressStream::new();
        let reporter = stream.progress_reporter().clone();
        for i in 0..100_000u64 {
            reporter.report(Progress {
                uploaded: i,
                upload_total: Some(100_000),
                downloaded: i,
                download_total: Some(100_000),
            });
        }
        match poll_next_once(&mut stream) {
            Poll::Ready(Some(got)) => {
                assert_eq!(got.uploaded, 99_999);
                assert_eq!(got.downloaded, 99_999);
            }
            _ => panic!("expected progress after many reports"),
        }
    }

    #[test]
    fn reports_from_non_tokio_thread() {
        // No Tokio runtime is present in this test; reporting happens on a
        // plain OS thread while consumption happens here.
        let mut stream = ProgressStream::new();
        let reporter = stream.progress_reporter().clone();
        let handle = std::thread::spawn(move || {
            for i in 1..=10u64 {
                reporter.report(Progress {
                    uploaded: i,
                    upload_total: Some(10),
                    downloaded: 0,
                    download_total: None,
                });
            }
        });
        handle.join().unwrap();
        let mut last = 0;
        while let Poll::Ready(Some(p)) = poll_next_once(&mut stream) {
            last = p.uploaded;
            if last == 10 {
                break;
            }
        }
        assert_eq!(last, 10);
    }

    #[test]
    fn no_tokio_handle_required() {
        // Ensures the channel works without any async runtime handle.
        // This test runs on a plain thread without Tokio.
        let mut stream = ProgressStream::new();
        let reporter = stream.progress_reporter().clone();
        reporter.report(Progress {
            uploaded: 5,
            upload_total: Some(5),
            downloaded: 5,
            download_total: Some(5),
        });
        match poll_next_once(&mut stream) {
            Poll::Ready(Some(p)) => assert_eq!(p.uploaded, 5),
            _ => panic!("expected progress without runtime"),
        }
    }

    #[test]
    fn stream_usable_after_request_completion() {
        // Simulates: upload reported during request execution, download
        // continues after the request future completes.
        let mut stream = ProgressStream::new();
        let reporter = stream.progress_reporter().clone();
        reporter.report(Progress {
            uploaded: 100,
            upload_total: Some(100),
            downloaded: 0,
            download_total: None,
        });
        // "Request future completes" here; stream must remain usable.
        match poll_next_once(&mut stream) {
            Poll::Ready(Some(p)) => assert_eq!(p.uploaded, 100),
            _ => panic!("expected upload progress"),
        }
        // Download progress continues while the body is consumed.
        reporter.report(Progress {
            uploaded: 100,
            upload_total: Some(100),
            downloaded: 50,
            download_total: Some(100),
        });
        match poll_next_once(&mut stream) {
            Poll::Ready(Some(p)) => assert_eq!(p.downloaded, 50),
            _ => panic!("expected download progress after request completion"),
        }
        drop(reporter);
        // Drain any remaining, then expect termination.
        while let Poll::Ready(Some(_)) = poll_next_once(&mut stream) {}
        match poll_next_once(&mut stream) {
            Poll::Ready(None) => {}
            _ => panic!("expected termination after reporter drop"),
        }
    }
}
