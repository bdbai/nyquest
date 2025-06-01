#![allow(non_camel_case_types)]
#![allow(non_snake_case)]

use std::io::{Read as _, SeekFrom};
use std::sync::atomic::Ordering;
use std::sync::Mutex;
use std::{io, sync::atomic::AtomicU64};

use nyquest_interface::blocking::BoxedStream;
use windows::{
    Foundation::IClosable_Impl,
    Storage::Streams::{
        IBuffer, IInputStream, IInputStream_Impl, IOutputStream_Impl, IRandomAccessStream,
        IRandomAccessStream_Impl, InputStreamOptions,
    },
    Web::Http::{HttpStreamContent, IHttpContent},
    Win32::{
        Foundation::{ERROR_CANCELLED, E_NOTIMPL},
        System::WinRT::IBufferByteAccess,
    },
};
use windows_core::*;
use windows_future::{IAsyncOperation, IAsyncOperationWithProgress};

#[interface("39aaaac2-7495-4cd4-9929-1d9e01d59934")]
unsafe trait IBlockingReadStreamBaseAccess: IUnknown {
    unsafe fn StreamBase(&self) -> *const BlockingReadStreamBase;
}

struct BlockingReadStreamBase {
    stream: Mutex<Option<BoxedStream>>,
    pos: AtomicU64,
}

#[implement(IBlockingReadStreamBaseAccess, IInputStream)]
struct BlockingReadInputStream(BlockingReadStreamBase);

#[implement(IBlockingReadStreamBaseAccess, IInputStream, IRandomAccessStream)]
struct BlockingReadRandomAccessStream(BlockingReadStreamBase);

unsafe impl Send for IBlockingReadStreamBaseAccess {}

impl BlockingReadStreamBase {
    // TODO: do work on caller thread
    fn read_async(
        &self,
        base_accessor: IBlockingReadStreamBaseAccess,
        buffer: windows_core::Ref<'_, IBuffer>,
        count: u32,
        options: InputStreamOptions,
    ) -> windows_core::Result<IAsyncOperationWithProgress<IBuffer, u32>> {
        let Some(buffer) = buffer.as_ref() else {
            return Err(windows_core::Error::empty());
        };
        let buffer = AgileReference::new(buffer)?;
        Ok(IAsyncOperationWithProgress::spawn(move || {
            let base = unsafe { &*base_accessor.StreamBase() };
            let Ok(mut stream) = base.stream.lock() else {
                return Err(windows_core::Error::new(
                    ERROR_CANCELLED.into(),
                    "poisoned mutex",
                ));
            };
            let Some(stream) = &mut *stream else {
                return Err(windows_core::Error::new(
                    ERROR_CANCELLED.into(),
                    "stream is None",
                ));
            };
            let buffer = AgileReference::resolve(&buffer)?;
            let to_read = if (options.0 & InputStreamOptions::ReadAhead.0) != 0 {
                buffer.Capacity()?
            } else {
                count.min(buffer.Capacity()?)
            } as usize;
            let iba = buffer.cast::<IBufferByteAccess>()?;
            unsafe {
                let buf = iba.Buffer()?;
                let buf = std::slice::from_raw_parts_mut(buf, to_read);
                let read_len = if options == InputStreamOptions::None {
                    stream.read_exact(buf).map_err(windows_core::Error::from)?;
                    to_read
                } else {
                    stream.read(buf).map_err(windows_core::Error::from)?
                };
                base.pos.fetch_add(read_len as u64, Ordering::Relaxed);
                buffer.SetLength(read_len as u32)?;
            }
            Ok(buffer)
        }))
    }

    fn close(&self) -> windows_core::Result<()> {
        if let Ok(mut stream) = self.stream.lock() {
            *stream = None;
        }
        Ok(())
    }
}

impl IBlockingReadStreamBaseAccess_Impl for BlockingReadInputStream_Impl {
    unsafe fn StreamBase(&self) -> *const BlockingReadStreamBase {
        &self.0
    }
}

impl IInputStream_Impl for BlockingReadInputStream_Impl {
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

impl IBlockingReadStreamBaseAccess_Impl for BlockingReadRandomAccessStream_Impl {
    unsafe fn StreamBase(&self) -> *const BlockingReadStreamBase {
        &self.0
    }
}

impl IInputStream_Impl for BlockingReadRandomAccessStream_Impl {
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

impl IOutputStream_Impl for BlockingReadRandomAccessStream_Impl {
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

impl IRandomAccessStream_Impl for BlockingReadRandomAccessStream_Impl {
    fn Size(&self) -> windows_core::Result<u64> {
        let stream = self
            .0
            .stream
            .lock()
            .map_err(|_| windows_core::Error::new(ERROR_CANCELLED.into(), "poisoned mutex"))?;
        match &*stream {
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
        let Some(BoxedStream::Sized { stream, .. }) = &mut *stream else {
            return Ok(());
        };
        stream
            .seek(SeekFrom::Start(0))
            .map_err(windows_core::Error::from)?;
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
        let Some(stream) = &*stream else {
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

impl IClosable_Impl for BlockingReadInputStream_Impl {
    fn Close(&self) -> windows_core::Result<()> {
        self.0.close()
    }
}

impl IClosable_Impl for BlockingReadRandomAccessStream_Impl {
    fn Close(&self) -> windows_core::Result<()> {
        self.0.close()
    }
}

pub(super) fn transform_stream(stream: BoxedStream) -> io::Result<IHttpContent> {
    let is_sized = match &stream {
        BoxedStream::Sized { .. } => true,
        BoxedStream::Unsized { .. } => false,
    };
    let base = BlockingReadStreamBase {
        stream: Mutex::new(Some(stream)),
        pos: AtomicU64::new(0),
    };
    let stream: IInputStream = if is_sized {
        BlockingReadRandomAccessStream(base)
            .into_object()
            .into_interface()
    } else {
        BlockingReadInputStream(base).into_object().into_interface()
    };
    let content = HttpStreamContent::CreateFromInputStream(&stream)?.cast()?;
    Ok(content)
}
