use std::future::Future;
use std::io;
use std::pin::Pin;
use std::sync::{Arc, Mutex, Weak};
use std::task::{Context, Poll, Waker};

use nyquest_interface::r#async::futures_io::AsyncRead;
use nyquest_interface::r#async::BoxedStream;
use windows::Win32::Foundation::{E_ILLEGAL_METHOD_CALL, S_OK};
use windows::{
    Foundation::IClosable_Impl,
    Storage::Streams::{IBuffer, IInputStream, IInputStream_Impl, InputStreamOptions},
    Web::Http::{HttpStreamContent, IHttpContent},
    Win32::{Foundation::ERROR_CANCELLED, System::WinRT::IBufferByteAccess},
};
use windows_core::{implement, AgileReference, ComObject, ComObjectInner, IUnknownImpl, Interface};
use windows_future::{
    AsyncOperationProgressHandler, AsyncOperationWithProgressCompletedHandler, AsyncStatus,
    IAsyncInfo_Impl, IAsyncOperationWithProgress, IAsyncOperationWithProgress_Impl,
};

#[implement(IInputStream)]
struct AsyncReadInputStream {
    stream: Mutex<Option<BoxedStream>>,
    task_collection: Weak<Mutex<StreamReadTaskCollectionInner>>,
}

#[implement(IAsyncOperationWithProgress<IBuffer, u32>)]
struct AsyncReadInputStreamReadTask {
    parent: ComObject<AsyncReadInputStream>,
    read_count: u32,
    options: InputStreamOptions,
    buffer: AgileReference<IBuffer>,

    inner: Mutex<AsyncReadInputStreamReadTaskInner>,
}

struct AsyncReadInputStreamReadTaskInner {
    status: Result<bool, windows_core::Error>,
    // progress: Option<AsyncOperationProgressHandler<IBuffer, u32>>,
    completed: Option<AgileReference<AsyncOperationWithProgressCompletedHandler<IBuffer, u32>>>,
}

impl IInputStream_Impl for AsyncReadInputStream_Impl {
    fn ReadAsync(
        &self,
        buffer: windows_core::Ref<'_, IBuffer>,
        count: u32,
        options: InputStreamOptions,
    ) -> windows_core::Result<IAsyncOperationWithProgress<IBuffer, u32>> {
        let this = self.to_object();
        let Some(buffer) = buffer.as_ref() else {
            return Err(windows_core::Error::empty());
        };
        let Some(task_collection) = self.task_collection.upgrade() else {
            return Err(windows_core::Error::new(
                ERROR_CANCELLED.into(),
                "task collection is None",
            ));
        };
        let Ok(mut task_collection) = task_collection.lock() else {
            return Err(windows_core::Error::new(
                ERROR_CANCELLED.into(),
                "poisoned task collection mutex",
            ));
        };
        let buffer = AgileReference::new(buffer)?;
        let task = AsyncReadInputStreamReadTask {
            parent: this,
            read_count: count,
            options,
            buffer: buffer.clone(),
            inner: Mutex::new(AsyncReadInputStreamReadTaskInner {
                status: Ok(false),
                completed: None,
            }),
        }
        .into_object();
        task_collection.tasks.push(task.clone());
        if let Some(waker) = task_collection.waker.as_mut() {
            waker.wake_by_ref();
        }
        Ok(task.into_interface())
    }
}

impl IClosable_Impl for AsyncReadInputStream_Impl {
    fn Close(&self) -> windows_core::Result<()> {
        if let Ok(mut stream) = self.stream.lock() {
            *stream = None;
        }
        Ok(())
    }
}

impl IAsyncOperationWithProgress_Impl<IBuffer, u32> for AsyncReadInputStreamReadTask_Impl {
    fn SetProgress(
        &self,
        _handler: windows_core::Ref<'_, AsyncOperationProgressHandler<IBuffer, u32>>,
    ) -> windows_core::Result<()> {
        Ok(())
    }

    fn Progress(&self) -> windows_core::Result<AsyncOperationProgressHandler<IBuffer, u32>> {
        Err(windows_core::Error::empty())
    }

    fn SetCompleted(
        &self,
        handler: windows_core::Ref<'_, AsyncOperationWithProgressCompletedHandler<IBuffer, u32>>,
    ) -> windows_core::Result<()> {
        if let Ok(mut inner) = self.inner.lock() {
            inner.completed = handler.as_ref().map(AgileReference::new).transpose()?;
            if let Some(handler) = handler.as_ref() {
                let status = match &inner.status {
                    Ok(true) => AsyncStatus::Completed,
                    Err(_) => AsyncStatus::Error,
                    Ok(false) => return Ok(()),
                };
                handler.Invoke(self.as_interface(), status)?;
            }
        }
        Ok(())
    }

    fn Completed(
        &self,
    ) -> windows_core::Result<AsyncOperationWithProgressCompletedHandler<IBuffer, u32>> {
        if let Ok(inner) = self.inner.lock() {
            if let Some(completed) = &inner.completed {
                return completed.resolve();
            }
        }
        Err(windows_core::Error::empty())
    }

    fn GetResults(&self) -> windows_core::Result<IBuffer> {
        let mut inner = self.inner.lock().unwrap();
        match std::mem::replace(&mut inner.status, Ok(false)) {
            Err(e) => Err(e),
            Ok(false) => Err(windows_core::Error::from_hresult(E_ILLEGAL_METHOD_CALL)),
            Ok(true) => {
                let buffer = AgileReference::resolve(&self.buffer)?;
                Ok(buffer)
            }
        }
    }
}

impl IAsyncInfo_Impl for AsyncReadInputStreamReadTask_Impl {
    fn Id(&self) -> windows_core::Result<u32> {
        Ok(1)
    }

    fn Status(&self) -> windows_core::Result<windows_future::AsyncStatus> {
        let inner = self.inner.lock().unwrap();
        match &inner.status {
            Ok(true) => Ok(AsyncStatus::Completed),
            Ok(false) => Ok(AsyncStatus::Started),
            Err(_) => Ok(AsyncStatus::Error),
        }
    }

    fn ErrorCode(&self) -> windows_core::Result<windows_core::HRESULT> {
        let inner = self.inner.lock().unwrap();
        if let Err(e) = inner.status.as_ref() {
            Ok(e.code())
        } else {
            Ok(S_OK)
        }
    }

    fn Cancel(&self) -> windows_core::Result<()> {
        let Some(task_collection) = self.parent.task_collection.upgrade() else {
            return Ok(());
        };
        let Ok(mut task_collection) = task_collection.lock() else {
            return Err(windows_core::Error::new(
                ERROR_CANCELLED.into(),
                "poisoned task collection mutex",
            ));
        };
        task_collection
            .tasks
            .retain(|task| !std::ptr::addr_eq(task.get(), self.get_impl()));
        Ok(())
    }

    fn Close(&self) -> windows_core::Result<()> {
        {
            let Ok(mut stream) = self.parent.stream.lock() else {
                return Ok(());
            };
            *stream = None;
        }
        self.Cancel()
    }
}

#[derive(Default)]
pub(super) struct StreamReadTaskCollectionInner {
    waker: Option<Waker>,
    tasks: Vec<ComObject<AsyncReadInputStreamReadTask>>,
}
#[derive(Clone, Default)]
pub(super) struct StreamReadTaskCollection {
    inner: Option<Arc<Mutex<StreamReadTaskCollectionInner>>>,
}

impl Future for StreamReadTaskCollection {
    type Output = ();

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.get_mut();
        let Some(task_collection) = this.inner.as_mut() else {
            return Poll::Pending;
        };
        let mut task_collection = task_collection.lock().unwrap();
        task_collection.waker = Some(cx.waker().clone());
        task_collection
            .tasks
            .retain(|task| match poll_retain_read_task(task, cx) {
                Ok(true) => true,
                Ok(false) => {
                    let mut task_inner = task.inner.lock().unwrap();
                    task_inner.status = Ok(true);
                    if let Some(completed) = task_inner.completed.take() {
                        let Ok(completed) = completed.resolve() else {
                            // FIXME:
                            return false;
                        };
                        completed
                            .Invoke(task.as_interface(), AsyncStatus::Completed)
                            .ok();
                    }
                    false
                }
                Err(e) => {
                    let mut task_inner = task.inner.lock().unwrap();
                    task_inner.status = Err(e);
                    if let Some(completed) = task_inner.completed.take() {
                        let Ok(completed) = completed.resolve() else {
                            // FIXME:
                            return false;
                        };
                        completed
                            .Invoke(task.as_interface(), AsyncStatus::Error)
                            .ok();
                    }
                    false
                }
            });
        Poll::Pending
    }
}

fn poll_retain_read_task(
    task: &ComObject<AsyncReadInputStreamReadTask>,
    cx: &mut Context<'_>,
) -> windows_core::Result<bool> {
    let buffer = AgileReference::resolve(&task.buffer)?;
    let to_read = if (task.options.0 & InputStreamOptions::ReadAhead.0) != 0 {
        buffer.Capacity()?
    } else {
        task.read_count.min(buffer.Capacity()?)
    } as usize;
    let offset = buffer.Length()? as usize;
    let mut stream = task.parent.stream.lock().unwrap();
    let Some(stream) = stream.as_mut() else {
        return Err(windows_core::Error::new(
            ERROR_CANCELLED.into(),
            "stream closed",
        ));
    };
    let iba = buffer.cast::<IBufferByteAccess>()?;
    unsafe {
        let buf = iba.Buffer()?.add(offset);
        let buf = std::slice::from_raw_parts_mut(buf, to_read);
        let read_res =
            AsyncRead::poll_read(stream.as_mut(), cx, buf).map_err(windows_core::Error::from)?;
        match read_res {
            Poll::Pending => Ok(true),
            Poll::Ready(read_len)
                if task.options == InputStreamOptions::None && read_len < to_read =>
            {
                buffer.SetLength((offset + read_len) as u32)?;
                Ok(true)
            }
            Poll::Ready(read_len) => {
                buffer.SetLength((offset + read_len) as u32)?;
                Ok(false)
            }
        }
    }
}

pub(super) fn transform_stream(
    stream: BoxedStream,
    task_collection: &mut StreamReadTaskCollection,
) -> io::Result<IHttpContent> {
    let task_collection = match &mut task_collection.inner {
        Some(task_collection) => task_collection,
        None => task_collection.inner.insert(Default::default()),
    };
    let stream = AsyncReadInputStream {
        stream: Mutex::new(Some(stream)),
        task_collection: Arc::downgrade(task_collection),
    }
    .into_object();
    let content =
        HttpStreamContent::CreateFromInputStream(&stream.to_interface::<IInputStream>())?.cast()?;
    Ok(content)
}
