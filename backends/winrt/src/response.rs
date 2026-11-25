use std::io;

use windows::core::HSTRING;
#[cfg(any(feature = "async-progress", feature = "blocking-progress"))]
use windows::Web::Http::{HttpProgress, HttpProgressStage};
use windows::Web::Http::{HttpResponseMessage, IHttpContent};
#[cfg(any(feature = "async-progress", feature = "blocking-progress"))]
use windows_core::RuntimeType;
#[cfg(any(feature = "async-progress", feature = "blocking-progress"))]
use windows_future::{AsyncOperationProgressHandler, IAsyncOperationWithProgress};

#[cfg(any(feature = "async-progress", feature = "blocking-progress"))]
use std::sync::{Arc, Mutex};

#[cfg(any(feature = "async-progress", feature = "blocking-progress"))]
use nyquest_interface::{Error as NyquestError, ProgressReporter, Result as NyquestResult};

#[cfg(any(feature = "async-progress", feature = "blocking-progress"))]
use crate::error::IntoNyquestResult;
#[cfg(any(feature = "async-progress", feature = "blocking-progress"))]
use crate::progress::{ProgressStage, WinrtProgressTracker};

use crate::timer::Timer;

pub struct WinrtResponse {
    pub(crate) status: u16,
    pub(crate) content_length: Option<u64>,
    pub(crate) max_response_buffer_size: Option<u64>,
    pub(crate) request_timer: Timer,
    pub(crate) response: HttpResponseMessage,
    /// Retains the progress sink (and thereby the reporter) for as long as
    /// body progress can be reported, i.e. through response body consumption.
    #[cfg(any(feature = "async-progress", feature = "blocking-progress"))]
    pub(crate) progress_sink: Option<Arc<WinrtProgressSink>>,
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
#[cfg(any(feature = "async-progress", feature = "blocking-progress"))]
pub(crate) struct WinrtProgressSink {
    state: Mutex<WinrtProgressSinkState>,
}

#[cfg(any(feature = "async-progress", feature = "blocking-progress"))]
struct WinrtProgressSinkState {
    tracker: WinrtProgressTracker,
    last_sent: u64,
    last_send_total: Option<u64>,
    last_retries: u32,
    body_downloaded: u64,
    body_total: Option<u64>,
    body_primed: bool,
}

#[cfg(any(feature = "async-progress", feature = "blocking-progress"))]
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
        let stage = if progress.Stage == HttpProgressStage::SendingHeaders {
            Some(ProgressStage::SendingHeaders)
        } else if progress.Stage == HttpProgressStage::SendingContent {
            Some(ProgressStage::SendingContent)
        } else if progress.Stage == HttpProgressStage::WaitingForResponse {
            Some(ProgressStage::WaitingForResponse)
        } else if progress.Stage == HttpProgressStage::ReceivingHeaders {
            Some(ProgressStage::ReceivingHeaders)
        } else if progress.Stage == HttpProgressStage::ReceivingContent {
            Some(ProgressStage::ReceivingContent)
        } else {
            None
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
            if cumulative > state.body_downloaded {
                state.body_downloaded = cumulative;
            }
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
#[cfg(any(feature = "async-progress", feature = "blocking-progress"))]
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
#[cfg(any(feature = "async-progress", feature = "blocking-progress"))]
#[must_use = "call assert_size to enforce response size limit"]
pub(crate) struct ProgressReadGuard {
    exceeded: Arc<std::sync::atomic::AtomicBool>,
    max_response_buffer_size: Option<u64>,
    sink: Option<Arc<WinrtProgressSink>>,
}

#[cfg(any(feature = "async-progress", feature = "blocking-progress"))]
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
#[cfg(any(feature = "async-progress", feature = "blocking-progress"))]
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

impl WinrtResponse {
    pub(crate) fn new(
        res: HttpResponseMessage,
        response_size_limit: Option<u64>,
        request_timer: Timer,
    ) -> io::Result<WinrtResponse> {
        let content_length = match res.Content() {
            Ok(content) => content
                .Headers()?
                .ContentLength()
                .ok()
                .and_then(|v| v.Value().ok()),
            Err(_) => Some(0),
        };
        Ok(WinrtResponse {
            status: res.StatusCode()?.0 as u16,
            content_length,
            max_response_buffer_size: response_size_limit,
            request_timer,
            response: res,
            #[cfg(any(feature = "async-progress", feature = "blocking-progress"))]
            progress_sink: None,
        })
    }

    pub(crate) fn get_header(&self, header: &str) -> io::Result<Vec<String>> {
        let headers = self.response.Headers()?;
        let header_name = HSTRING::from(header);
        let mut headers = headers.Lookup(&header_name).ok();
        if headers.is_none() {
            headers = self.content()?.Headers()?.Lookup(&header_name).ok();
        }
        Ok(headers.into_iter().map(|h| h.to_string_lossy()).collect())
    }

    pub(crate) fn content(&self) -> io::Result<IHttpContent> {
        Ok(self.response.Content()?)
    }
}
