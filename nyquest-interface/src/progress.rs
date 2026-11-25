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

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    #[test]
    fn field_semantics() {
        let default = Progress::default();
        assert_eq!(default.uploaded, 0);
        assert_eq!(default.upload_total, None);
        assert_eq!(default.downloaded, 0);
        assert_eq!(default.download_total, None);

        let known_zero = Progress {
            uploaded: 0,
            upload_total: Some(0),
            downloaded: 0,
            download_total: Some(0),
        };
        assert_eq!(known_zero.upload_total, Some(0));
        assert_eq!(known_zero.download_total, Some(0));

        let known = Progress {
            uploaded: 1024,
            upload_total: Some(4096),
            downloaded: 2048,
            download_total: Some(8192),
        };
        assert_eq!(known.uploaded, 1024);
        assert_eq!(known.upload_total, Some(4096));
        assert_eq!(known.downloaded, 2048);
        assert_eq!(known.download_total, Some(8192));

        let unknown = Progress {
            uploaded: 10,
            upload_total: None,
            downloaded: 20,
            download_total: None,
        };
        assert_eq!(unknown.upload_total, None);
        assert_eq!(unknown.download_total, None);

        // Complete snapshots contain both directions.
        let snapshot = Progress {
            uploaded: 1024,
            upload_total: Some(4096),
            downloaded: 2048,
            download_total: Some(8192),
        };
        let copied = snapshot;
        assert_eq!(snapshot, copied);
    }

    #[test]
    fn fn_reporter_impl() {
        let seen = Mutex::new(Vec::new());
        let reporter = |p: Progress| {
            seen.lock().unwrap().push(p);
        };
        let progress = Progress {
            uploaded: 1,
            upload_total: Some(2),
            downloaded: 3,
            download_total: Some(4),
        };
        ProgressReporter::report(&reporter, progress);
        assert_eq!(seen.lock().unwrap().as_slice(), &[progress]);
    }

    #[test]
    fn arc_reporter_forwards() {
        struct Collect {
            inner: Mutex<Vec<Progress>>,
        }
        impl ProgressReporter for Collect {
            fn report(&self, progress: Progress) {
                self.inner.lock().unwrap().push(progress);
            }
        }
        let collect = Arc::new(Collect {
            inner: Mutex::new(vec![]),
        });
        let arc: Arc<dyn ProgressReporter> = collect.clone();
        let progress = Progress {
            uploaded: 5,
            upload_total: None,
            downloaded: 6,
            download_total: None,
        };
        arc.report(progress);
        // `Arc<dyn ProgressReporter>` itself implements `ProgressReporter`.
        let arc2: Arc<dyn ProgressReporter> = arc.clone();
        ProgressReporter::report(&arc2, progress);
        let stored = collect.inner.lock().unwrap();
        assert_eq!(stored.as_slice(), &[progress, progress]);
    }

    #[cfg(all(feature = "async", feature = "async-progress"))]
    mod async_forwarding {
        use super::*;
        use crate::r#async::{AnyAsyncClient, AsyncClient, AsyncResponse, Request};
        use crate::Result;
        use std::pin::Pin;

        struct MockResponse;
        impl AsyncResponse for MockResponse {
            fn status(&self) -> u16 {
                200
            }
            fn content_length(&self) -> Option<u64> {
                None
            }
            fn get_header(&self, _header: &str) -> Result<Vec<String>> {
                Ok(vec![])
            }
            async fn text(self: Pin<&mut Self>) -> Result<String> {
                Ok(String::new())
            }
            async fn bytes(self: Pin<&mut Self>) -> Result<Vec<u8>> {
                Ok(vec![])
            }
        }
        #[cfg(feature = "async-stream")]
        impl futures_io::AsyncRead for MockResponse {
            fn poll_read(
                self: Pin<&mut Self>,
                _cx: &mut std::task::Context<'_>,
                _buf: &mut [u8],
            ) -> std::task::Poll<std::io::Result<usize>> {
                std::task::Poll::Ready(Ok(0))
            }
        }

        #[derive(Clone)]
        struct MockClient {
            seen: Arc<Mutex<Option<Progress>>>,
        }

        impl AsyncClient for MockClient {
            type Response = MockResponse;
            async fn request(&self, _req: Request) -> Result<Self::Response> {
                Ok(MockResponse)
            }
            async fn request_with_progress(
                &self,
                _req: Request,
                progress: Arc<dyn ProgressReporter>,
            ) -> Result<Self::Response> {
                progress.report(Progress {
                    uploaded: 7,
                    upload_total: Some(7),
                    downloaded: 8,
                    download_total: Some(8),
                });
                *self.seen.lock().unwrap() = Some(Progress {
                    uploaded: 7,
                    upload_total: Some(7),
                    downloaded: 8,
                    download_total: Some(8),
                });
                Ok(MockResponse)
            }

            #[cfg(feature = "cookies")]
            async fn add_cookie(
                &self,
                _name: std::borrow::Cow<'static, str>,
                _value: std::borrow::Cow<'static, str>,
                _url: std::borrow::Cow<'static, str>,
            ) -> Result<()> {
                Ok(())
            }

            #[cfg(feature = "cookies")]
            async fn get_cookies_for(
                &self,
                _url: std::borrow::Cow<'static, str>,
            ) -> Result<Vec<crate::Cookie>> {
                Ok(vec![])
            }
        }

        #[test]
        fn any_async_forwards_reporter() {
            use std::task::{Context, Poll, RawWaker, RawWakerVTable, Waker};
            fn noop_waker() -> Waker {
                fn clone(_: *const ()) -> RawWaker {
                    RawWaker::new(std::ptr::null(), &VTABLE)
                }
                fn noop(_: *const ()) {}
                static VTABLE: RawWakerVTable = RawWakerVTable::new(clone, noop, noop, noop);
                unsafe { Waker::from_raw(RawWaker::new(std::ptr::null(), &VTABLE)) }
            }
            let seen = Arc::new(Mutex::new(None));
            let client = MockClient { seen: seen.clone() };
            let client: Arc<dyn AnyAsyncClient> = Arc::new(client);
            let req = Request {
                method: crate::Method::Get,
                relative_uri: "http://example.com/".into(),
                additional_headers: vec![],
                body: None,
            };
            struct Collect {
                inner: Mutex<Vec<Progress>>,
            }
            impl ProgressReporter for Collect {
                fn report(&self, p: Progress) {
                    self.inner.lock().unwrap().push(p);
                }
            }
            let collect = Arc::new(Collect {
                inner: Mutex::new(vec![]),
            });
            let reporter: Arc<dyn ProgressReporter> = collect.clone();
            let waker = noop_waker();
            let mut cx = Context::from_waker(&waker);
            let mut fut = client.request_with_progress(req, reporter);
            match fut.as_mut().poll(&mut cx) {
                Poll::Ready(Ok(_)) => {}
                Poll::Ready(Err(_)) => panic!("mock should be immediately ready"),
                Poll::Pending => panic!("mock should be immediately ready"),
            }
            // The mock reported through the forwarded reporter.
            let got = collect.inner.lock().unwrap();
            assert_eq!(got.len(), 1);
            assert_eq!(got[0].uploaded, 7);
            assert!(seen.lock().unwrap().is_some());
        }

        #[derive(Clone)]
        struct DefaultClient;
        impl AsyncClient for DefaultClient {
            type Response = MockResponse;
            async fn request(&self, _req: Request) -> Result<Self::Response> {
                Ok(MockResponse)
            }
            // Uses default `request_with_progress` which delegates to `request`.

            #[cfg(feature = "cookies")]
            async fn add_cookie(
                &self,
                _name: std::borrow::Cow<'static, str>,
                _value: std::borrow::Cow<'static, str>,
                _url: std::borrow::Cow<'static, str>,
            ) -> Result<()> {
                Ok(())
            }

            #[cfg(feature = "cookies")]
            async fn get_cookies_for(
                &self,
                _url: std::borrow::Cow<'static, str>,
            ) -> Result<Vec<crate::Cookie>> {
                Ok(vec![])
            }
        }

        #[test]
        fn default_async_ignores_reporter() {
            let client = DefaultClient;
            let client: Arc<dyn AnyAsyncClient> = Arc::new(client);
            let req = Request {
                method: crate::Method::Get,
                relative_uri: "http://example.com/".into(),
                additional_headers: vec![],
                body: None,
            };
            struct Noop;
            impl ProgressReporter for Noop {
                fn report(&self, _p: Progress) {}
            }
            let reporter: Arc<dyn ProgressReporter> = Arc::new(Noop);
            use std::task::{Context, Poll, RawWaker, RawWakerVTable, Waker};
            fn noop_waker() -> Waker {
                fn clone(_: *const ()) -> RawWaker {
                    RawWaker::new(std::ptr::null(), &VTABLE)
                }
                fn noop(_: *const ()) {}
                static VTABLE: RawWakerVTable = RawWakerVTable::new(clone, noop, noop, noop);
                unsafe { Waker::from_raw(RawWaker::new(std::ptr::null(), &VTABLE)) }
            }
            let waker = noop_waker();
            let mut cx = Context::from_waker(&waker);
            let mut fut = client.request_with_progress(req, reporter);
            match fut.as_mut().poll(&mut cx) {
                Poll::Ready(Ok(_)) => {}
                Poll::Ready(Err(_)) => panic!("expected ready Ok, got Err"),
                Poll::Pending => panic!("expected ready Ok, got Pending"),
            }
        }
    }

    #[cfg(all(feature = "blocking", feature = "blocking-progress"))]
    mod blocking_forwarding {
        use super::*;
        use crate::blocking::{AnyBlockingClient, BlockingClient, BlockingResponse, Request};
        use crate::Result;

        struct MockResponse;
        impl BlockingResponse for MockResponse {
            fn status(&self) -> u16 {
                200
            }
            fn content_length(&self) -> Option<u64> {
                None
            }
            fn get_header(&self, _header: &str) -> Result<Vec<String>> {
                Ok(vec![])
            }
            fn text(&mut self) -> Result<String> {
                Ok(String::new())
            }
            fn bytes(&mut self) -> Result<Vec<u8>> {
                Ok(vec![])
            }
        }
        #[cfg(feature = "blocking-stream")]
        impl std::io::Read for MockResponse {
            fn read(&mut self, _buf: &mut [u8]) -> std::io::Result<usize> {
                Ok(0)
            }
        }

        #[derive(Clone)]
        struct MockClient;
        impl BlockingClient for MockClient {
            type Response = MockResponse;
            fn request(&self, _req: Request) -> Result<Self::Response> {
                Ok(MockResponse)
            }
            fn request_with_progress(
                &self,
                _req: Request,
                progress: Arc<dyn ProgressReporter>,
            ) -> Result<Self::Response> {
                progress.report(Progress {
                    uploaded: 1,
                    upload_total: Some(1),
                    downloaded: 2,
                    download_total: Some(2),
                });
                Ok(MockResponse)
            }

            #[cfg(feature = "cookies")]
            fn add_cookie(
                &self,
                _name: std::borrow::Cow<'static, str>,
                _value: std::borrow::Cow<'static, str>,
                _url: std::borrow::Cow<'static, str>,
            ) -> Result<()> {
                Ok(())
            }

            #[cfg(feature = "cookies")]
            fn get_cookies_for(
                &self,
                _url: std::borrow::Cow<'static, str>,
            ) -> Result<Vec<crate::Cookie>> {
                Ok(vec![])
            }
        }

        #[test]
        fn any_blocking_forwards_reporter() {
            let client: Arc<dyn AnyBlockingClient> = Arc::new(MockClient);
            let req = Request {
                method: crate::Method::Get,
                relative_uri: "http://example.com/".into(),
                additional_headers: vec![],
                body: None,
            };
            struct Collect {
                inner: Mutex<Vec<Progress>>,
            }
            impl ProgressReporter for Collect {
                fn report(&self, p: Progress) {
                    self.inner.lock().unwrap().push(p);
                }
            }
            let collect = Arc::new(Collect {
                inner: Mutex::new(vec![]),
            });
            let reporter: Arc<dyn ProgressReporter> = collect.clone();
            client.request_with_progress(req, reporter).unwrap();
            let got = collect.inner.lock().unwrap();
            assert_eq!(got.len(), 1);
            assert_eq!(got[0].uploaded, 1);
        }

        #[derive(Clone)]
        struct DefaultClient;
        impl BlockingClient for DefaultClient {
            type Response = MockResponse;
            fn request(&self, _req: Request) -> Result<Self::Response> {
                Ok(MockResponse)
            }

            #[cfg(feature = "cookies")]
            fn add_cookie(
                &self,
                _name: std::borrow::Cow<'static, str>,
                _value: std::borrow::Cow<'static, str>,
                _url: std::borrow::Cow<'static, str>,
            ) -> Result<()> {
                Ok(())
            }

            #[cfg(feature = "cookies")]
            fn get_cookies_for(
                &self,
                _url: std::borrow::Cow<'static, str>,
            ) -> Result<Vec<crate::Cookie>> {
                Ok(vec![])
            }
        }

        #[test]
        fn default_blocking_ignores_reporter() {
            let client: Arc<dyn AnyBlockingClient> = Arc::new(DefaultClient);
            let req = Request {
                method: crate::Method::Get,
                relative_uri: "http://example.com/".into(),
                additional_headers: vec![],
                body: None,
            };
            struct Noop;
            impl ProgressReporter for Noop {
                fn report(&self, _p: Progress) {}
            }
            client.request_with_progress(req, Arc::new(Noop)).unwrap();
        }
    }
}
