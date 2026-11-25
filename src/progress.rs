//! Progress reporting implementations for the `nyquest` facade.
//!
//! Backends produce progress via [`ProgressReporter`]. This module implements
//! delivery to users: a callback-backed reporter for blocking clients and a
//! channel-backed reporter with [`ProgressStream`] for async clients.
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

#[cfg(any(feature = "async-progress", feature = "blocking-progress"))]
use nyquest_interface::{Progress, ProgressReporter};

/// Callback-backed reporter for blocking progress reporting.
///
/// The callback is invoked synchronously from [`ProgressReporter::report`],
/// which for blocking transfers runs on the calling thread.
#[cfg(feature = "blocking-progress")]
pub(crate) struct CallbackProgressReporter<F> {
    callback: F,
}

#[cfg(feature = "blocking-progress")]
impl<F> CallbackProgressReporter<F>
where
    F: Fn(Progress) + Send + Sync,
{
    pub(crate) fn new(callback: F) -> Self {
        Self { callback }
    }
}

#[cfg(feature = "blocking-progress")]
impl<F> ProgressReporter for CallbackProgressReporter<F>
where
    F: Fn(Progress) + Send + Sync,
{
    fn report(&self, progress: Progress) {
        (self.callback)(progress);
    }
}

#[cfg(feature = "async-progress")]
use std::sync::{Arc, Mutex};
#[cfg(feature = "async-progress")]
use std::task::{Context, Poll, Waker};

#[cfg(feature = "async-progress")]
#[derive(Debug, Default)]
struct ProgressSharedInner {
    latest: Progress,
    version: u64,
    closed: bool,
    waker: Option<Waker>,
}

#[cfg(feature = "async-progress")]
#[derive(Debug)]
struct ProgressShared {
    inner: Mutex<ProgressSharedInner>,
}

/// Channel-backed reporter for async progress reporting.
///
/// Accepts reports from arbitrary backend threads, updates thread-safe shared
/// state, and wakes the [`ProgressStream`] consumer when new progress is
/// available. Intermediate notifications are coalesced: only the latest
/// snapshot is retained, rather than accumulating an unbounded queue.
///
/// Does not execute user code and does not depend on any async runtime.
#[cfg(feature = "async-progress")]
#[derive(Debug)]
pub(crate) struct ChannelProgressReporter {
    shared: Arc<ProgressShared>,
}

#[cfg(feature = "async-progress")]
impl ChannelProgressReporter {
    fn with_shared(shared: Arc<ProgressShared>) -> Self {
        Self { shared }
    }
}

#[cfg(feature = "async-progress")]
impl ProgressReporter for ChannelProgressReporter {
    fn report(&self, progress: Progress) {
        let waker = {
            let mut inner = self.shared.inner.lock().unwrap_or_else(|e| e.into_inner());
            inner.latest = progress;
            inner.version = inner.version.wrapping_add(1);
            inner.waker.take()
        };
        if let Some(waker) = waker {
            waker.wake();
        }
    }
}

#[cfg(feature = "async-progress")]
impl Drop for ChannelProgressReporter {
    fn drop(&mut self) {
        let waker = {
            let mut inner = self.shared.inner.lock().unwrap_or_else(|e| e.into_inner());
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
#[cfg(feature = "async-progress")]
#[derive(Debug)]
pub struct ProgressStream {
    shared: Arc<ProgressShared>,
    seen_version: u64,
}

#[cfg(feature = "async-progress")]
impl ProgressStream {
    pub(crate) fn new() -> (ChannelProgressReporter, Self) {
        let shared = Arc::new(ProgressShared {
            inner: Mutex::new(ProgressSharedInner::default()),
        });
        let reporter = ChannelProgressReporter::with_shared(shared.clone());
        let stream = Self {
            shared,
            seen_version: 0,
        };
        (reporter, stream)
    }
}

#[cfg(feature = "async-progress")]
impl futures_core::Stream for ProgressStream {
    type Item = Progress;

    fn poll_next(self: std::pin::Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = self.get_mut();
        let mut inner = this.shared.inner.lock().unwrap_or_else(|e| e.into_inner());
        if inner.version != this.seen_version {
            this.seen_version = inner.version;
            let latest = inner.latest;
            Poll::Ready(Some(latest))
        } else if inner.closed {
            Poll::Ready(None)
        } else {
            inner.waker = Some(cx.waker().clone());
            Poll::Pending
        }
    }
}

#[cfg(all(test, feature = "blocking-progress"))]
mod blocking_tests {
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

#[cfg(all(test, feature = "async-progress"))]
mod async_tests {
    use super::*;
    use futures_core::Stream;
    use std::task::{Context, Poll, RawWaker, RawWakerVTable, Waker};

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
        let (reporter, mut stream) = ProgressStream::new();
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
        let (reporter, mut stream) = ProgressStream::new();
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
        let (reporter, mut stream) = ProgressStream::new();
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
        let (reporter, stream) = ProgressStream::new();
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
        let (reporter, mut stream) = ProgressStream::new();
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
        let (reporter, mut stream) = ProgressStream::new();
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
        let (reporter, mut stream) = ProgressStream::new();
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
        let (reporter, mut stream) = ProgressStream::new();
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
