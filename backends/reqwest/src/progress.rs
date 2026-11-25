use std::sync::{Arc, Mutex};

use nyquest_interface::{Progress, ProgressReporter};

/// Tracks upload and download byte counts and reports complete snapshots.
///
/// Every report is a complete [`Progress`] snapshot containing both upload and
/// download counters. Intermediate notifications may be coalesced by the
/// consumer; this tracker simply reports the latest snapshot on each update.
pub(crate) struct ProgressTracker {
    reporter: Arc<dyn ProgressReporter>,
    state: Mutex<Progress>,
}

impl ProgressTracker {
    pub fn new(reporter: Arc<dyn ProgressReporter>, upload_total: Option<u64>) -> Arc<Self> {
        let tracker = Arc::new(Self {
            reporter,
            state: Mutex::new(Progress {
                uploaded: 0,
                upload_total,
                downloaded: 0,
                download_total: None,
            }),
        });
        tracker.report_current();
        tracker
    }

    fn report_current(&self) {
        let snapshot = *self.state.lock().unwrap_or_else(|e| e.into_inner());
        self.reporter.report(snapshot);
    }

    pub fn set_download_total(&self, total: Option<u64>) {
        {
            let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
            state.download_total = total;
        }
        self.report_current();
    }

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
}

/// Body wrapper that reports upload progress as bytes are consumed by reqwest.
///
/// If the body is not a stream, it jumps from 0 to 100%: the entire content is
/// reported once it is polled.
pub(crate) struct TrackingBytesBody {
    data: Option<bytes::Bytes>,
    progress: Option<Arc<ProgressTracker>>,
}

impl TrackingBytesBody {
    pub fn new(data: bytes::Bytes, progress: Option<Arc<ProgressTracker>>) -> Self {
        Self {
            data: Some(data),
            progress,
        }
    }
}

impl http_body::Body for TrackingBytesBody {
    type Data = bytes::Bytes;
    type Error = std::io::Error;

    fn poll_frame(
        self: std::pin::Pin<&mut Self>,
        _cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Result<http_body::Frame<Self::Data>, Self::Error>>> {
        let this = self.get_mut();
        if let Some(data) = this.data.take() {
            if let Some(progress) = &this.progress {
                progress.add_uploaded(data.len() as u64);
            }
            std::task::Poll::Ready(Some(Ok(http_body::Frame::data(data))))
        } else {
            std::task::Poll::Ready(None)
        }
    }

    fn is_end_stream(&self) -> bool {
        self.data.is_none()
    }

    fn size_hint(&self) -> http_body::SizeHint {
        if let Some(data) = &self.data {
            http_body::SizeHint::with_exact(data.len() as u64)
        } else {
            http_body::SizeHint::with_exact(0)
        }
    }
}

/// Computes the expected upload total for a request body, if known.
#[cfg(all(feature = "async", feature = "async-stream"))]
pub fn upload_total_for_async_body(
    body: &Option<nyquest_interface::Body<nyquest_interface::r#async::BoxedStream>>,
) -> Option<u64> {
    match body {
        None => None,
        Some(nyquest_interface::Body::Bytes { content, .. }) => {
            let len = content.len() as u64;
            if len == 0 {
                None
            } else {
                Some(len)
            }
        }
        Some(nyquest_interface::Body::Form { .. }) => None,
        #[cfg(feature = "multipart")]
        Some(nyquest_interface::Body::Multipart { .. }) => None,
        Some(nyquest_interface::Body::Stream { stream, .. }) => match stream {
            nyquest_interface::r#async::BoxedStream::Sized { content_length, .. } => {
                if *content_length == 0 {
                    None
                } else {
                    Some(*content_length)
                }
            }
            nyquest_interface::r#async::BoxedStream::Unsized { .. } => None,
        },
    }
}

/// Computes the expected upload total for an async request body when stream
/// bodies are not enabled.
#[cfg(all(feature = "async", not(feature = "async-stream")))]
pub fn upload_total_for_async_body<S>(body: &Option<nyquest_interface::Body<S>>) -> Option<u64> {
    match body {
        Some(nyquest_interface::Body::Bytes { content, .. }) => {
            let len = content.len() as u64;
            if len == 0 {
                None
            } else {
                Some(len)
            }
        }
        _ => None,
    }
}

/// Computes the expected upload total for a blocking request body, if known.
#[cfg(all(feature = "blocking", feature = "blocking-stream"))]
pub fn upload_total_for_blocking_body(
    body: &Option<nyquest_interface::Body<nyquest_interface::blocking::BoxedStream>>,
) -> Option<u64> {
    match body {
        None => None,
        Some(nyquest_interface::Body::Bytes { content, .. }) => {
            let len = content.len() as u64;
            if len == 0 {
                None
            } else {
                Some(len)
            }
        }
        Some(nyquest_interface::Body::Form { .. }) => None,
        #[cfg(feature = "multipart")]
        Some(nyquest_interface::Body::Multipart { .. }) => None,
        Some(nyquest_interface::Body::Stream { stream, .. }) => match stream {
            nyquest_interface::blocking::BoxedStream::Sized { content_length, .. } => {
                if *content_length == 0 {
                    None
                } else {
                    Some(*content_length)
                }
            }
            nyquest_interface::blocking::BoxedStream::Unsized { .. } => None,
        },
    }
}

/// Computes the expected upload total for a blocking request body without
/// stream support.
#[cfg(all(feature = "blocking", not(feature = "blocking-stream")))]
pub fn upload_total_for_blocking_body<S>(body: &Option<nyquest_interface::Body<S>>) -> Option<u64> {
    match body {
        Some(nyquest_interface::Body::Bytes { content, .. }) => {
            let len = content.len() as u64;
            if len == 0 {
                None
            } else {
                Some(len)
            }
        }
        _ => None,
    }
}

/// Normalizes a content-length style total: zero means unknown.
pub fn normalize_total(total: u64) -> Option<u64> {
    if total == 0 {
        None
    } else {
        Some(total)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
        let tracker = ProgressTracker::new(collect.clone(), Some(100));
        tracker.add_uploaded(40);
        tracker.set_download_total(Some(200));
        tracker.add_downloaded(10);
        let reports = collect.inner.lock().unwrap();
        assert!(!reports.is_empty());
        let last = reports.last().unwrap();
        assert_eq!(last.uploaded, 40);
        assert_eq!(last.upload_total, Some(100));
        assert_eq!(last.downloaded, 10);
        assert_eq!(last.download_total, Some(200));
    }

    #[test]
    fn zero_total_is_unknown() {
        assert_eq!(normalize_total(0), None);
        assert_eq!(normalize_total(10), Some(10));
    }
}
