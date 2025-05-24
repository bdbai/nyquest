use std::io::SeekFrom;
use std::sync::atomic::Ordering;
use std::sync::Mutex;
use std::{io, sync::atomic::AtomicU64};

use nyquest_interface::{blocking::BoxedStream, SizedStream};
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
use windows_core::{implement, AgileReference, IUnknownImpl, Interface};
use windows_future::{IAsyncOperation, IAsyncOperationWithProgress};

#[implement(IInputStream, IRandomAccessStream)]
struct BlockingReadInputStream {
    stream: Mutex<Option<SizedStream<BoxedStream>>>,
    pos: AtomicU64,
}

// TODO: do work on caller thread
impl IInputStream_Impl for BlockingReadInputStream_Impl {
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
        let buffer = AgileReference::new(buffer)?;
        Ok(IAsyncOperationWithProgress::spawn(move || {
            let Ok(mut stream) = this.stream.lock() else {
                return Err(windows_core::Error::new(
                    ERROR_CANCELLED.into(),
                    "poisoned mutex",
                ));
            };
            let Some(SizedStream { stream, .. }) = &mut *stream else {
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
                this.pos.fetch_add(read_len as u64, Ordering::Relaxed);
                buffer.SetLength(read_len as u32)?;
            }
            Ok(buffer)
        }))
    }
}

impl IOutputStream_Impl for BlockingReadInputStream_Impl {
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

impl IRandomAccessStream_Impl for BlockingReadInputStream_Impl {
    fn Size(&self) -> windows_core::Result<u64> {
        let stream = self
            .stream
            .lock()
            .map_err(|_| windows_core::Error::new(ERROR_CANCELLED.into(), "poisoned mutex"))?;
        if let Some(SizedStream {
            content_length: Some(len),
            ..
        }) = &*stream
        {
            Ok(*len)
        } else {
            Ok(0)
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
        let pos = self.pos.load(Ordering::Relaxed);
        Ok(pos)
    }

    fn Seek(&self, position: u64) -> windows_core::Result<()> {
        if position != 0 {
            return Err(windows_core::Error::from_hresult(E_NOTIMPL));
        }
        let Ok(mut stream) = self.stream.lock() else {
            return Err(windows_core::Error::new(
                ERROR_CANCELLED.into(),
                "poisoned mutex",
            ));
        };
        let Some(SizedStream { stream, .. }) = &mut *stream else {
            return Ok(());
        };
        stream
            .seek(SeekFrom::Start(0))
            .map_err(windows_core::Error::from)?;
        self.pos.store(0, Ordering::Relaxed);
        Ok(())
    }

    fn CloneStream(&self) -> windows_core::Result<IRandomAccessStream> {
        Err(windows_core::Error::from_hresult(E_NOTIMPL))
    }

    fn CanRead(&self) -> windows_core::Result<bool> {
        let Ok(stream) = self.stream.lock() else {
            return Ok(false);
        };
        let Some(SizedStream { content_length, .. }) = &*stream else {
            return Ok(false);
        };
        if let Some(len) = content_length {
            if self.pos.load(Ordering::Relaxed) >= *len {
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
        if let Ok(mut stream) = self.stream.lock() {
            *stream = None;
        }
        Ok(())
    }
}

pub(super) fn transform_stream(stream: SizedStream<BoxedStream>) -> io::Result<IHttpContent> {
    let content =
        HttpStreamContent::CreateFromInputStream(&IInputStream::from(BlockingReadInputStream {
            stream: Mutex::new(Some(stream)),
            pos: AtomicU64::new(0),
        }))?
        .cast()?;
    Ok(content)
}
