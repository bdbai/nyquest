#[cfg(feature = "async-stream")]
use std::future::IntoFuture;
use std::io;
use std::pin::{pin, Pin};

use nyquest_interface::client::ClientOptions;
use nyquest_interface::r#async::{AsyncBackend, AsyncClient, AsyncResponse, Request};
use nyquest_interface::Result as NyquestResult;
use windows::Web::Http::HttpCompletionOption;
#[cfg(feature = "async-stream")]
use windows_future::IAsyncOperation;

#[cfg(feature = "async-stream")]
mod stream_content;
mod timer_ext;

use crate::client::WinrtClient;
use crate::error::IntoNyquestResult;
use crate::ibuffer::IBufferExt;
use crate::request::create_body;
use crate::response::WinrtResponse;
use crate::response_size_limiter::ResponseSizeLimiter;
use crate::timer::Timer;
use timer_ext::AsyncTimeoutExt;

pub struct WinrtAsyncResponse {
    inner: WinrtResponse,
    #[cfg(feature = "async-stream")]
    load_data_task: Option<<IAsyncOperation<u32> as IntoFuture>::IntoFuture>,
}

impl crate::WinrtBackend {
    pub fn create_async_client(&self, options: ClientOptions) -> io::Result<WinrtClient> {
        WinrtClient::create(options)
    }
}

impl WinrtClient {
    async fn send_request_async(&self, req: Request) -> NyquestResult<WinrtAsyncResponse> {
        let req_msg = self.create_request(&req)?;
        #[cfg(feature = "async-stream")]
        let mut stream_tasks = Default::default();
        #[cfg(not(feature = "async-stream"))]
        let stream_tasks = std::future::pending::<()>();
        if let Some(body) = req.body {
            let body = create_body(body, &mut |s| {
                #[cfg(feature = "async-stream")]
                {
                    stream_content::transform_stream(s, &mut stream_tasks)
                }
                #[cfg(not(feature = "async-stream"))]
                {
                    let _ = s;
                    unreachable!("async-stream feature is disabled")
                }
            })?;
            self.append_content_headers(&body, &req.additional_headers)?;
            req_msg.SetContent(&body).into_nyquest_result()?;
        }
        let mut timer = Timer::new(self.request_timeout);
        let res = {
            let request_fut = pin!(self
                .client
                .SendRequestWithOptionAsync(&req_msg, HttpCompletionOption::ResponseHeadersRead)
                .into_nyquest_result()?
                .timeout_by(&mut timer));
            match futures_util::future::select(request_fut, stream_tasks).await {
                futures_util::future::Either::Left((res, _)) => res,
                futures_util::future::Either::Right((_, _)) => unreachable!(),
            }?
        };
        let inner =
            WinrtResponse::new(res, self.max_response_buffer_size, timer).into_nyquest_result()?;
        Ok(WinrtAsyncResponse {
            inner,
            #[cfg(feature = "async-stream")]
            load_data_task: None,
        })
    }
}

impl AsyncResponse for WinrtAsyncResponse {
    fn status(&self) -> u16 {
        self.inner.status
    }

    fn content_length(&self) -> Option<u64> {
        self.inner.content_length
    }

    fn get_header(&self, header: &str) -> nyquest_interface::Result<Vec<String>> {
        self.inner.get_header(header).into_nyquest_result()
    }

    async fn text(mut self: Pin<&mut Self>) -> nyquest_interface::Result<String> {
        let task = self
            .inner
            .response
            .Content()
            .into_nyquest_result()?
            .ReadAsStringAsync()
            .into_nyquest_result()?;
        let size_limiter =
            ResponseSizeLimiter::hook_progress(self.inner.max_response_buffer_size, &task)?;
        let res = task
            .timeout_by(&mut self.inner.request_timer)
            .await
            .map(|r| r.to_string_lossy());
        let content = size_limiter.assert_size(res)?;
        Ok(content)
    }

    async fn bytes(mut self: Pin<&mut Self>) -> nyquest_interface::Result<Vec<u8>> {
        let task = self
            .inner
            .response
            .Content()
            .into_nyquest_result()?
            .ReadAsBufferAsync()
            .into_nyquest_result()?;
        let size_limiter =
            ResponseSizeLimiter::hook_progress(self.inner.max_response_buffer_size, &task)?;
        let res = task
            .timeout_by(&mut self.inner.request_timer)
            .await
            .and_then(|b| b.to_vec());
        let arr = size_limiter.assert_size(res)?;
        Ok(arr)
    }
}

#[cfg(feature = "async-stream")]
impl nyquest_interface::r#async::futures_io::AsyncRead for WinrtAsyncResponse {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut [u8],
    ) -> std::task::Poll<io::Result<usize>> {
        use std::future::Future as _;
        use std::task::{ready, Poll};

        let this = self.get_mut();
        loop {
            if let Some(load_data_task) = this.load_data_task.as_mut() {
                let loaded = ready!(Pin::new(load_data_task).poll(cx)?);
                this.load_data_task = None;
                if loaded == 0 {
                    return Poll::Ready(Ok(0));
                }
            }

            let reader = this.inner.reader_mut()?;
            let size = reader.UnconsumedBufferLength()?;
            if size == 0 {
                this.load_data_task = Some(reader.LoadAsync(buf.len() as u32)?.into_future());
                continue;
            }
            let size = buf.len().min(size as usize);
            let buf = &mut buf[..size];
            reader.ReadBytes(buf)?;
            break Poll::Ready(Ok(size));
        }
    }
}

impl AsyncClient for WinrtClient {
    type Response = WinrtAsyncResponse;

    async fn request(&self, req: Request) -> nyquest_interface::Result<Self::Response> {
        self.send_request_async(req).await
    }
}

impl AsyncBackend for crate::WinrtBackend {
    type AsyncClient = WinrtClient;

    async fn create_async_client(
        &self,
        options: ClientOptions,
    ) -> NyquestResult<Self::AsyncClient> {
        self.create_async_client(options).into_nyquest_result()
    }
}
