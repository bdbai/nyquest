#[cfg(test)]
mod tests {
    #[cfg(all(feature = "blocking", feature = "blocking-progress"))]
    use std::sync::{Arc, Mutex};

    #[cfg(any(feature = "blocking-progress", feature = "async-progress"))]
    use http_body_util::{Full, StreamBody};
    #[cfg(any(feature = "blocking-progress", feature = "async-progress"))]
    use hyper::body::{Bytes, Frame};
    #[cfg(any(feature = "blocking-progress", feature = "async-progress"))]
    use nyquest::Request as NyquestRequest;

    #[cfg(all(feature = "blocking", feature = "blocking-progress"))]
    use nyquest::blocking::Body as NyquestBlockingBody;
    #[cfg(all(feature = "async", feature = "async-progress"))]
    use nyquest::r#async::Body as NyquestAsyncBody;

    // Used for `TOKIO_RT` in async tests; blocking tests refer to the
    // fixture helpers via qualified `crate::` paths.
    #[cfg(all(feature = "async", feature = "async-progress"))]
    use crate::*;

    #[cfg(all(feature = "blocking", feature = "blocking-progress"))]
    fn collect_blocking() -> (
        Arc<Mutex<Vec<nyquest::Progress>>>,
        impl Fn(nyquest::Progress) + Send + Sync + 'static,
    ) {
        let collected: Arc<Mutex<Vec<nyquest::Progress>>> = Arc::new(Mutex::new(vec![]));
        let cloned = collected.clone();
        let cb = move |p: nyquest::Progress| {
            cloned.lock().unwrap().push(p);
        };
        (collected, cb)
    }

    #[cfg(all(feature = "blocking", feature = "blocking-progress"))]
    #[test]
    fn test_blocking_download_progress() {
        const PATH: &str = "progress/blocking_download";
        const BODY: &[u8] = &[0xAB; 8192];
        let _handle = crate::add_hyper_fixture(PATH, |req| async move {
            let res = hyper::Response::new(Full::new(Bytes::from_static(BODY)));
            (
                res,
                (req.method() == hyper::Method::GET)
                    .then_some(())
                    .ok_or(req),
            )
        });
        let builder = crate::init_builder_blocking().unwrap();
        let client = builder.build_blocking().unwrap();
        let (collected, cb) = collect_blocking();
        let res = client
            .request_with_progress(NyquestRequest::get(PATH), cb)
            .unwrap()
            .bytes()
            .unwrap();
        assert_eq!(res, BODY);
        let reports = collected.lock().unwrap();
        assert!(!reports.is_empty(), "expected download progress reports");
        let last = reports.last().unwrap();
        assert_eq!(last.downloaded as usize, BODY.len());
        // Known total should be reported when content-length is present.
        assert!(
            reports
                .iter()
                .any(|p| p.download_total == Some(BODY.len() as u64)),
            "expected download_total Some(len), got {reports:?}"
        );
        // Complete snapshots contain both directions.
        assert!(reports.iter().all(|p| p.uploaded == 0
            || p.upload_total.is_none()
            || p.upload_total == Some(0)
            || p.uploaded <= p.upload_total.unwrap_or(u64::MAX)));
        let _ = last;
    }

    #[cfg(all(feature = "blocking", feature = "blocking-progress"))]
    #[test]
    fn test_blocking_upload_progress() {
        const PATH: &str = "progress/blocking_upload";
        const UPLOAD_LEN: usize = 32 * 1024;
        let _handle = crate::add_hyper_fixture(PATH, |req| async move {
            use http_body_util::BodyExt as _;
            let body = req.into_body().collect().await.unwrap().to_bytes();
            assert_eq!(body.len(), UPLOAD_LEN);
            let res = hyper::Response::new(Full::new(Bytes::from_static(b"ok")));
            (res, Ok(()))
        });
        let builder = crate::init_builder_blocking().unwrap();
        let client = builder.build_blocking().unwrap();
        let data = vec![0xCD; UPLOAD_LEN];
        let body = NyquestBlockingBody::bytes(data, "application/octet-stream");
        let (collected, cb) = collect_blocking();
        let res = client
            .request_with_progress(NyquestRequest::post(PATH).with_body(body), cb)
            .unwrap()
            .text()
            .unwrap();
        assert_eq!(res, "ok");
        let reports = collected.lock().unwrap();
        assert!(!reports.is_empty());
        let max_uploaded = reports.iter().map(|p| p.uploaded).max().unwrap();
        assert_eq!(max_uploaded as usize, UPLOAD_LEN);
        assert!(
            reports
                .iter()
                .any(|p| p.upload_total == Some(UPLOAD_LEN as u64)),
            "expected upload_total Some(len), got {reports:?}"
        );
    }

    #[cfg(all(feature = "blocking", feature = "blocking-progress"))]
    #[test]
    fn test_blocking_unknown_totals() {
        const PATH: &str = "progress/blocking_unknown_totals";
        let _handle = crate::add_hyper_fixture(PATH, |_req| async move {
            // Streamed body without content-length => unknown total.
            let (mut tx, rx) =
                futures::channel::mpsc::channel::<Result<Frame<Bytes>, hyper::Error>>(1);
            tokio::spawn(async move {
                use futures::SinkExt as _;
                let _ = tx
                    .send(Ok(Frame::data(Bytes::from_static(b"chunk1"))))
                    .await;
                let _ = tx
                    .send(Ok(Frame::data(Bytes::from_static(b"chunk2"))))
                    .await;
            });
            let body = http_body_util::BodyExt::boxed(StreamBody::new(rx));
            let res = hyper::Response::new(body);
            (res, Ok(()))
        });
        let builder = crate::init_builder_blocking().unwrap();
        let client = builder.build_blocking().unwrap();
        let (collected, cb) = collect_blocking();
        let res = client
            .request_with_progress(NyquestRequest::get(PATH), cb)
            .unwrap()
            .bytes()
            .unwrap();
        assert_eq!(res, b"chunk1chunk2");
        let reports = collected.lock().unwrap();
        assert!(!reports.is_empty());
        assert!(
            reports.iter().any(|p| p.download_total.is_none()),
            "expected unknown download total, got {reports:?}"
        );
        assert!(reports.iter().any(|p| p.downloaded > 0));
    }

    #[cfg(all(feature = "blocking", feature = "blocking-progress"))]
    #[test]
    fn test_blocking_combined_snapshots() {
        const PATH: &str = "progress/blocking_combined";
        const UPLOAD_LEN: usize = 4096;
        const DOWNLOAD_BODY: &[u8] = &[0xEF; 4096];
        let _handle = crate::add_hyper_fixture(PATH, |req| async move {
            use http_body_util::BodyExt as _;
            let _ = req.into_body().collect().await.unwrap();
            let res = hyper::Response::new(Full::new(Bytes::from_static(DOWNLOAD_BODY)));
            (res, Ok(()))
        });
        let builder = crate::init_builder_blocking().unwrap();
        let client = builder.build_blocking().unwrap();
        let body = NyquestBlockingBody::bytes(vec![0x11; UPLOAD_LEN], "application/octet-stream");
        let (collected, cb) = collect_blocking();
        let res = client
            .request_with_progress(NyquestRequest::post(PATH).with_body(body), cb)
            .unwrap()
            .bytes()
            .unwrap();
        assert_eq!(res, DOWNLOAD_BODY);
        let reports = collected.lock().unwrap();
        assert!(!reports.is_empty());
        // Every report is a complete snapshot (struct always has both directions).
        let last = reports.last().unwrap();
        assert_eq!(last.uploaded as usize, UPLOAD_LEN);
        assert_eq!(last.downloaded as usize, DOWNLOAD_BODY.len());
    }

    #[cfg(all(feature = "async", feature = "async-progress"))]
    #[test]
    fn test_async_upload_and_download_progress() {
        use futures_util::StreamExt as _;
        const PATH: &str = "progress/async_upload_download";
        const UPLOAD_LEN: usize = 32 * 1024;
        const DOWNLOAD_BODY: &[u8] = &[0xAA; 8192];
        let _handle = crate::add_hyper_fixture(PATH, |req| async move {
            use http_body_util::BodyExt as _;
            let body = req.into_body().collect().await.unwrap().to_bytes();
            assert_eq!(body.len(), UPLOAD_LEN);
            let res = hyper::Response::new(Full::new(Bytes::from_static(DOWNLOAD_BODY)));
            (res, Ok(()))
        });
        TOKIO_RT.block_on(async {
            let builder = crate::init_builder().await.unwrap();
            let client = builder.build_async().await.unwrap();
            let data = vec![0xCD; UPLOAD_LEN];
            let body = NyquestAsyncBody::bytes(data, "application/octet-stream");
            let (request, mut progress) =
                client.request_with_progress(NyquestRequest::post(PATH).with_body(body));
            tokio::pin!(request);
            let mut upload_seen = false;
            let mut download_seen_early = false;
            let response = loop {
                tokio::select! {
                    result = &mut request => break result.unwrap(),
                    Some(p) = progress.next() => {
                        if p.uploaded > 0 {
                            upload_seen = true;
                        }
                        if p.downloaded > 0 {
                            download_seen_early = true;
                        }
                    }
                }
            };
            // Drain any trailing report: on fast transports the whole
            // transfer may complete before the first poll, leaving the
            // request future and a pending progress snapshot ready in the
            // same round.
            if !upload_seen {
                tokio::select! {
                    Some(p) = progress.next() => {
                        if p.uploaded > 0 {
                            upload_seen = true;
                        }
                        if p.downloaded > 0 {
                            download_seen_early = true;
                        }
                    }
                    _ = tokio::time::sleep(std::time::Duration::from_millis(100)) => {}
                }
            }
            assert!(
                upload_seen,
                "expected upload progress during request execution"
            );
            // Stream remains usable after request completion; download continues
            // while the body is consumed. Exercise async work while consuming.
            // Note: for small bodies the download may already have completed
            // during request execution, so accept progress from either phase.
            let body_fut = response.bytes();
            tokio::pin!(body_fut);
            let mut download_seen = download_seen_early;
            let bytes = loop {
                tokio::select! {
                    result = &mut body_fut => break result.unwrap(),
                    Some(p) = progress.next() => {
                        if p.downloaded > 0 {
                            download_seen = true;
                        }
                        // User can perform async work while consuming progress.
                        tokio::task::yield_now().await;
                    }
                }
            };
            assert_eq!(bytes, DOWNLOAD_BODY);
            // Drain any trailing report (e.g. a final cumulative snapshot that
            // raced with body completion) before asserting.
            if !download_seen {
                tokio::select! {
                    Some(p) = progress.next() => {
                        if p.downloaded > 0 {
                            download_seen = true;
                        }
                    }
                    _ = tokio::time::sleep(std::time::Duration::from_millis(100)) => {}
                }
            }
            assert!(
                download_seen,
                "expected download progress during body consumption"
            );
        });
    }

    #[cfg(all(feature = "async", feature = "async-progress"))]
    #[test]
    fn test_async_unknown_totals_and_lifecycle() {
        use futures_util::StreamExt as _;
        const PATH: &str = "progress/async_unknown_and_lifecycle";
        let _handle = crate::add_hyper_fixture(PATH, |_req| async move {
            let (mut tx, rx) =
                futures::channel::mpsc::channel::<Result<Frame<Bytes>, hyper::Error>>(1);
            tokio::spawn(async move {
                use futures::SinkExt as _;
                let _ = tx
                    .send(Ok(Frame::data(Bytes::from_static(b"hello "))))
                    .await;
                let _ = tx.send(Ok(Frame::data(Bytes::from_static(b"world")))).await;
            });
            let body = http_body_util::BodyExt::boxed(StreamBody::new(rx));
            let res = hyper::Response::new(body);
            (res, Ok(()))
        });
        TOKIO_RT.block_on(async {
            let builder = crate::init_builder().await.unwrap();
            let client = builder.build_async().await.unwrap();
            let (request, mut progress) = client.request_with_progress(NyquestRequest::get(PATH));
            tokio::pin!(request);
            let response = request.await.unwrap();
            // Download with unknown total; body consumption continues reporting.
            // Box the body future so dropping it releases `response` (which
            // `bytes()` takes by value) and its progress reporter.
            let mut body_fut = Box::pin(response.bytes());
            let mut saw_unknown = false;
            let mut saw_bytes = false;
            let bytes = loop {
                tokio::select! {
                    result = &mut body_fut => break result.unwrap(),
                    Some(p) = progress.next() => {
                        if p.download_total.is_none() {
                            saw_unknown = true;
                        }
                        if p.downloaded > 0 {
                            saw_bytes = true;
                        }
                    }
                }
            };
            assert_eq!(bytes, b"hello world");
            // Drain any trailing report (e.g. a final cumulative snapshot that
            // raced with body completion) before asserting.
            if !saw_bytes || !saw_unknown {
                tokio::select! {
                    Some(p) = progress.next() => {
                        if p.download_total.is_none() {
                            saw_unknown = true;
                        }
                        if p.downloaded > 0 {
                            saw_bytes = true;
                        }
                    }
                    _ = tokio::time::sleep(std::time::Duration::from_millis(100)) => {}
                }
            }
            assert!(saw_bytes);
            assert!(saw_unknown, "expected unknown download total");
            // After body completion and response drop, the stream terminates.
            // (Reporter is retained through body consumption and dropped with
            // the response. `bytes()` takes `response` by value, so dropping
            // the completed body future drops the response and its reporter.)
            drop(body_fut);
            let mut terminated = false;
            for _ in 0..100 {
                tokio::select! {
                    opt = progress.next() => {
                        if opt.is_none() {
                            terminated = true;
                            break;
                        }
                    }
                    _ = tokio::time::sleep(std::time::Duration::from_millis(10)) => {
                        break;
                    }
                }
            }
            assert!(
                terminated,
                "expected progress stream to terminate after completion"
            );
        });
    }

    #[cfg(all(feature = "async", feature = "async-progress"))]
    #[test]
    fn test_async_stream_dropped_before_completion() {
        const PATH: &str = "progress/async_stream_drop";
        const BODY: &[u8] = &[0x55; 4096];
        let _handle = crate::add_hyper_fixture(PATH, |_req| async move {
            let res = hyper::Response::new(Full::new(Bytes::from_static(BODY)));
            (res, Ok(()))
        });
        TOKIO_RT.block_on(async {
            let builder = crate::init_builder().await.unwrap();
            let client = builder.build_async().await.unwrap();
            let (request, progress) = client.request_with_progress(NyquestRequest::get(PATH));
            // Dropping the stream before completion must be safe; the request
            // and body consumption still succeed.
            drop(progress);
            let response = request.await.unwrap();
            let bytes = response.bytes().await.unwrap();
            assert_eq!(bytes, BODY);
        });
    }

    #[cfg(all(feature = "async", feature = "async-progress"))]
    #[test]
    fn test_async_response_dropped_before_completion() {
        use futures_util::StreamExt as _;
        const PATH: &str = "progress/async_response_drop";
        let _handle = crate::add_hyper_fixture(PATH, |_req| async move {
            let (mut tx, rx) =
                futures::channel::mpsc::channel::<Result<Frame<Bytes>, hyper::Error>>(1);
            tokio::spawn(async move {
                use futures::SinkExt as _;
                // Send enough chunks that dropping early is meaningful.
                for _ in 0..100 {
                    if tx
                        .send(Ok(Frame::data(Bytes::from_static(&[0x99; 1024]))))
                        .await
                        .is_err()
                    {
                        break;
                    }
                    tokio::time::sleep(std::time::Duration::from_millis(5)).await;
                }
            });
            let body = http_body_util::BodyExt::boxed(StreamBody::new(rx));
            let res = hyper::Response::new(body);
            (res, Ok(()))
        });
        TOKIO_RT.block_on(async {
            let builder = crate::init_builder().await.unwrap();
            let client = builder.build_async().await.unwrap();
            let (request, mut progress) = client.request_with_progress(NyquestRequest::get(PATH));
            tokio::pin!(request);
            let response = loop {
                tokio::select! {
                    result = &mut request => break result.unwrap(),
                    Some(_) = progress.next() => {}
                }
            };
            // Drop the response before the body completes; the progress stream
            // must terminate rather than hang.
            drop(response);
            let mut terminated = false;
            for _ in 0..200 {
                tokio::select! {
                    opt = progress.next() => {
                        if opt.is_none() {
                            terminated = true;
                            break;
                        }
                    }
                    _ = tokio::time::sleep(std::time::Duration::from_millis(10)) => {
                        // Keep polling; backend cleanup is asynchronous.
                    }
                }
            }
            assert!(
                terminated,
                "expected progress stream to terminate after response drop"
            );
        });
    }

    #[cfg(all(feature = "async", feature = "async-progress"))]
    #[test]
    fn test_async_no_tokio_handle_and_cross_thread() {
        // `request_with_progress` itself does not require a Tokio handle, and
        // backend threads (non-Tokio) report progress consumed in Tokio tasks.
        // Here the request is created outside any Tokio context assertion and
        // consumed inside the runtime.
        const PATH: &str = "progress/async_no_handle";
        const BODY: &[u8] = b"handle";
        let _handle = crate::add_hyper_fixture(PATH, |_req| async move {
            let res = hyper::Response::new(Full::new(Bytes::from_static(BODY)));
            (res, Ok(()))
        });
        // Create the request future + stream without entering Tokio: the facade
        // must not require a runtime handle.
        let (client, req) = TOKIO_RT.block_on(async {
            let builder = crate::init_builder().await.unwrap();
            let client = builder.build_async().await.unwrap();
            (client, NyquestRequest::get(PATH))
        });
        let (request, mut progress) = client.request_with_progress(req);
        TOKIO_RT.block_on(async {
            use futures_util::StreamExt as _;
            tokio::pin!(request);
            // Either branch completing the loop proves progress can be
            // consumed inside a Tokio task while the request (whose reporter
            // lives on backend threads) completes.
            loop {
                tokio::select! {
                    result = &mut request => {
                        let response = result.unwrap();
                        let bytes = response.bytes().await.unwrap();
                        assert_eq!(bytes, BODY);
                        break;
                    }
                    Some(p) = progress.next() => {
                        // Progress reported from backend (non-Tokio) threads is
                        // consumed here inside a Tokio task.
                        let _ = p;
                        // Allow the request to complete.
                    }
                }
            }
        });
    }
}
