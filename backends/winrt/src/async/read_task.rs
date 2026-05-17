use std::{
    future::{Future as _, IntoFuture},
    io,
    pin::Pin,
    task::{Context, Poll},
};

use windows::{
    Storage::Streams::{DataReader, IInputStream, InputStreamOptions},
    Web::Http::IHttpContent,
};
use windows_core::AgileReference;
use windows_future::{IAsyncOperation, IAsyncOperationWithProgress};

type ReadAsStreamFuture =
    <IAsyncOperationWithProgress<IInputStream, u64> as IntoFuture>::IntoFuture;
type LoadDataFuture = <IAsyncOperation<u32> as IntoFuture>::IntoFuture;

pub(super) enum ReadTask {
    NotStarted(Option<AgileReference<IHttpContent>>),
    PollingStream(ReadAsStreamFuture),
    LoadingData(DataReader, LoadDataFuture),
    ConsumingData(DataReader),
}

impl ReadTask {
    fn dummy() -> Self {
        Self::NotStarted(None)
    }

    pub(super) fn new(content: Option<&IHttpContent>) -> Self {
        Self::NotStarted(content.and_then(|c| AgileReference::new(c).ok()))
    }
}

impl nyquest_interface::r#async::futures_io::AsyncRead for ReadTask {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut [u8],
    ) -> Poll<io::Result<usize>> {
        loop {
            let this = std::mem::replace(&mut *self, Self::dummy());
            match this {
                Self::NotStarted(None) => return Poll::Ready(Ok(0)),
                Self::NotStarted(Some(content)) => {
                    let content = content.resolve()?;
                    let task = content.ReadAsInputStreamAsync()?.into_future();
                    *self = Self::PollingStream(task);
                }
                Self::PollingStream(mut task) => match Pin::new(&mut task).poll(cx) {
                    Poll::Pending => {
                        *self = Self::PollingStream(task);
                        return Poll::Pending;
                    }
                    Poll::Ready(Err(e)) => return Poll::Ready(Err(e.into())),
                    Poll::Ready(Ok(stream)) => {
                        let reader = DataReader::CreateDataReader(&stream)?;
                        reader.SetInputStreamOptions(InputStreamOptions::Partial)?;
                        *self = Self::ConsumingData(reader);
                    }
                },
                Self::LoadingData(reader, mut task) => match Pin::new(&mut task).poll(cx) {
                    Poll::Pending => {
                        *self = Self::LoadingData(reader, task);
                        return Poll::Pending;
                    }
                    Poll::Ready(Err(e)) => return Poll::Ready(Err(e.into())),
                    Poll::Ready(Ok(0)) => return Poll::Ready(Ok(0)),
                    Poll::Ready(Ok(_)) => {
                        *self = Self::ConsumingData(reader);
                    }
                },
                Self::ConsumingData(reader) => {
                    let size = reader.UnconsumedBufferLength()?;
                    if size == 0 {
                        let load_data_task = reader.LoadAsync(buf.len() as u32)?.into_future();
                        *self = Self::LoadingData(reader, load_data_task);
                        continue;
                    }
                    let size = buf.len().min(size as usize);
                    let buf = &mut buf[..size];
                    reader.ReadBytes(buf)?;
                    *self = Self::ConsumingData(reader);
                    return Poll::Ready(Ok(size));
                }
            }
        }
    }
}
