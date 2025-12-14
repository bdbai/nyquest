#![allow(non_camel_case_types)]
#![allow(non_snake_case)]

use std::future::Future;
use std::io::{self, SeekFrom};
use std::pin::Pin;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, Weak};
use std::task::{Context, Poll, Waker};

use futures_util::AsyncRead as _;
use nyquest_interface::r#async::BoxedStream;
use windows::{
    Foundation::IClosable_Impl,
    Storage::Streams::{
        IBuffer, IInputStream, IInputStream_Impl, IOutputStream_Impl, IRandomAccessStream,
        IRandomAccessStream_Impl, InputStreamOptions,
    },
    Web::Http::{HttpStreamContent, IHttpContent},
    Win32::{
        Foundation::{ERROR_CANCELLED, E_ILLEGAL_METHOD_CALL, E_NOTIMPL, S_OK},
        System::WinRT::IBufferByteAccess,
    },
};
use windows_core::*;
use windows_future::{
    AsyncOperationProgressHandler, AsyncOperationWithProgressCompletedHandler, AsyncStatus,
    IAsyncInfo_Impl, IAsyncOperation, IAsyncOperationWithProgress,
    IAsyncOperationWithProgress_Impl,
};

#[interface("8ec308ec-a6ab-44c2-bff4-ed275ae0af68")]
unsafe trait IAsyncReadStreamBaseAccess: IUnknown {
    unsafe fn StreamBase(&self) -> *const AsyncReadStreamBase;
}

struct AsyncReadStreamBase {
    stream: Mutex<Option<(BoxedStream, bool)>>,
    task_collection: Weak<Mutex<StreamReadTaskCollectionInner>>,
    pos: AtomicU64,
}

#[implement(IAsyncReadStreamBaseAccess, IInputStream)]
struct AsyncReadInputStream(AsyncReadStreamBase);

#[implement(IAsyncReadStreamBaseAccess, IInputStream, IRandomAccessStream)]
struct AsyncReadRandomAccessStream(AsyncReadStreamBase);

unsafe impl Send for IAsyncReadStreamBaseAccess {}

#[implement(IAsyncOperationWithProgress<IBuffer, u32>)]
struct AsyncReadInputStreamReadTask {
    base_accessor: IAsyncReadStreamBaseAccess,
    read_count: u32,
    options: InputStreamOptions,
    buffer: AgileReference<IBuffer>,

    inner: Mutex<AsyncReadInputStreamReadTaskInner>,
}

struct AsyncReadInputStreamReadTaskInner {
    status: Result<bool>,
    // progress: Option<AsyncOperationProgressHandler<IBuffer, u32>>,
    completed: Option<AgileReference<AsyncOperationWithProgressCompletedHandler<IBuffer, u32>>>,
}

impl AsyncReadStreamBase {
    fn read_async(
        &self,
        base_accessor: IAsyncReadStreamBaseAccess,
        buffer: windows_core::Ref<'_, IBuffer>,
        count: u32,
        options: InputStreamOptions,
    ) -> windows_core::Result<IAsyncOperationWithProgress<IBuffer, u32>> {
        let Some(buffer) = buffer.as_ref() else {
            return Err(windows_core::Error::empty());
        };
        buffer.SetLength(0)?;
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
            base_accessor,
            read_count: count,
            options,
            buffer,
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

    fn close(&self) -> windows_core::Result<()> {
        if let Ok(mut stream) = self.stream.lock() {
            *stream = None;
        }
        Ok(())
    }
}

impl IAsyncReadStreamBaseAccess_Impl for AsyncReadInputStream_Impl {
    unsafe fn StreamBase(&self) -> *const AsyncReadStreamBase {
        &self.0
    }
}

impl IInputStream_Impl for AsyncReadInputStream_Impl {
    fn ReadAsync(
        &self,
        buffer: windows_core::Ref<'_, IBuffer>,
        count: u32,
        options: InputStreamOptions,
    ) -> windows_core::Result<IAsyncOperationWithProgress<IBuffer, u32>> {
        let base_accessor = self.to_interface();
        self.0.read_async(base_accessor, buffer, count, options)
    }
}

impl IAsyncReadStreamBaseAccess_Impl for AsyncReadRandomAccessStream_Impl {
    unsafe fn StreamBase(&self) -> *const AsyncReadStreamBase {
        &self.0
    }
}

impl IInputStream_Impl for AsyncReadRandomAccessStream_Impl {
    fn ReadAsync(
        &self,
        buffer: windows_core::Ref<'_, IBuffer>,
        count: u32,
        options: InputStreamOptions,
    ) -> windows_core::Result<IAsyncOperationWithProgress<IBuffer, u32>> {
        let base_accessor = self.to_interface();
        self.0.read_async(base_accessor, buffer, count, options)
    }
}

impl IOutputStream_Impl for AsyncReadRandomAccessStream_Impl {
    fn WriteAsync(
        &self,
        _buffer: windows_core::Ref<'_, IBuffer>,
    ) -> windows_core::Result<IAsyncOperationWithProgress<u32, u32>> {
        Err(windows_core::Error::from_hresult(E_NOTIMPL))
    }

    fn FlushAsync(&self) -> windows_core::Result<IAsyncOperation<bool>> {
        Err(windows_core::Error::from_hresult(E_NOTIMPL))
    }
}

impl IRandomAccessStream_Impl for AsyncReadRandomAccessStream_Impl {
    fn Size(&self) -> windows_core::Result<u64> {
        let stream = self
            .0
            .stream
            .lock()
            .map_err(|_| windows_core::Error::new(ERROR_CANCELLED.into(), "poisoned mutex"))?;
        match stream.as_ref().map(|s| &s.0) {
            Some(BoxedStream::Sized { content_length, .. }) => Ok(*content_length),
            _ => Err(E_NOTIMPL.into()),
        }
    }

    fn SetSize(&self, _value: u64) -> windows_core::Result<()> {
        Err(windows_core::Error::from_hresult(E_NOTIMPL))
    }

    fn GetInputStreamAt(&self, _position: u64) -> windows_core::Result<IInputStream> {
        Err(windows_core::Error::from_hresult(E_NOTIMPL))
    }

    fn GetOutputStreamAt(
        &self,
        _position: u64,
    ) -> windows_core::Result<windows::Storage::Streams::IOutputStream> {
        Err(windows_core::Error::from_hresult(E_NOTIMPL))
    }

    fn Position(&self) -> windows_core::Result<u64> {
        let pos = self.0.pos.load(Ordering::Relaxed);
        Ok(pos)
    }

    fn Seek(&self, position: u64) -> windows_core::Result<()> {
        if position != 0 {
            return Err(windows_core::Error::from_hresult(E_NOTIMPL));
        }
        let Ok(mut stream) = self.0.stream.lock() else {
            return Err(windows_core::Error::new(
                ERROR_CANCELLED.into(),
                "poisoned mutex",
            ));
        };
        let Some((.., need_rewind)) = &mut *stream else {
            return Ok(());
        };
        *need_rewind = true;
        self.0.pos.store(0, Ordering::Relaxed);
        Ok(())
    }

    fn CloneStream(&self) -> windows_core::Result<IRandomAccessStream> {
        Err(windows_core::Error::from_hresult(E_NOTIMPL))
    }

    fn CanRead(&self) -> windows_core::Result<bool> {
        let Ok(stream) = self.0.stream.lock() else {
            return Ok(false);
        };
        let Some((stream, _)) = &*stream else {
            return Ok(false);
        };
        if let BoxedStream::Sized { content_length, .. } = stream {
            if self.0.pos.load(Ordering::Relaxed) >= *content_length {
                return Ok(false);
            }
        }
        Ok(true)
    }

    fn CanWrite(&self) -> windows_core::Result<bool> {
        Ok(false)
    }
}

impl IClosable_Impl for AsyncReadInputStream_Impl {
    fn Close(&self) -> windows_core::Result<()> {
        self.0.close()
    }
}

impl IClosable_Impl for AsyncReadRandomAccessStream_Impl {
    fn Close(&self) -> windows_core::Result<()> {
        self.0.close()
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
        let base = unsafe { &*self.base_accessor.StreamBase() };
        let Some(task_collection) = base.task_collection.upgrade() else {
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
            let base = unsafe { &*self.base_accessor.StreamBase() };
            let Ok(mut stream) = base.stream.lock() else {
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
    let base = unsafe { &*task.base_accessor.StreamBase() };
    let mut stream = base.stream.lock().unwrap();
    let Some((stream, need_rewind)) = &mut *stream else {
        return Err(windows_core::Error::new(
            ERROR_CANCELLED.into(),
            "stream closed",
        ));
    };
    if let BoxedStream::Sized { stream, .. } = stream {
        if *need_rewind {
            if stream
                .as_mut()
                .poll_seek(cx, SeekFrom::Start(0))?
                .is_pending()
            {
                return Ok(true);
            } else {
                *need_rewind = false;
            }
        }
    }
    let buffer = AgileReference::resolve(&task.buffer)?;
    let offset = buffer.Length()? as usize;
    let capacity = buffer.Capacity()? as usize;
    let to_read = if (task.options.0 & InputStreamOptions::ReadAhead.0) != 0 {
        capacity
    } else {
        (task.read_count as usize).min(capacity)
    } - offset;
    let iba = buffer.cast::<IBufferByteAccess>()?;
    unsafe {
        let buf = iba.Buffer()?;
        let mut buf = &mut std::slice::from_raw_parts_mut(buf, capacity)[offset..][..to_read];
        let mut total_read_len = 0;
        let mut is_eof = false;
        while !buf.is_empty() {
            let read_res = Pin::new(&mut *stream)
                .poll_read(cx, buf)
                .map_err(windows_core::Error::from)?;
            if let Poll::Ready(read_len) = read_res {
                total_read_len += read_len;
                if read_len == 0 {
                    is_eof = true;
                } else if task.options != InputStreamOptions::Partial {
                    buf = &mut buf[read_len..];
                    continue;
                }
            }
            break;
        }
        let new_length = offset + total_read_len;
        buffer.SetLength(new_length as u32)?;
        base.pos.fetch_add(total_read_len as u64, Ordering::Relaxed);
        Ok(total_read_len < to_read && !is_eof)
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
    let is_sized = match &stream {
        BoxedStream::Unsized { .. } => false,
        BoxedStream::Sized { .. } => true,
    };
    let base = AsyncReadStreamBase {
        stream: Mutex::new(Some((stream, false))),
        task_collection: Arc::downgrade(task_collection),
        pos: AtomicU64::new(0),
    };
    let stream: IInputStream = if is_sized {
        AsyncReadRandomAccessStream(base)
            .into_object()
            .into_interface()
    } else {
        AsyncReadInputStream(base).into_object().into_interface()
    };
    let content = HttpStreamContent::CreateFromInputStream(&stream)?.cast()?;
    Ok(content)
}
