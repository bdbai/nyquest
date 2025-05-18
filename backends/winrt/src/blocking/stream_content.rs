use std::io;
use std::sync::Mutex;

use nyquest_interface::blocking::BoxedStream;
use windows::{
    Foundation::IClosable_Impl,
    Storage::Streams::{IBuffer, IInputStream, IInputStream_Impl, InputStreamOptions},
    Web::Http::{HttpStreamContent, IHttpContent},
    Win32::{Foundation::ERROR_CANCELLED, System::WinRT::IBufferByteAccess},
};
use windows_core::{implement, AgileReference, IUnknownImpl, Interface};
use windows_future::IAsyncOperationWithProgress;

#[implement(IInputStream)]
struct BlockingReadInputStream {
    stream: Mutex<Option<BoxedStream>>,
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
            let offset = buffer.Length()? as usize;
            let iba = buffer.cast::<IBufferByteAccess>()?;
            unsafe {
                let buf = iba.Buffer()?.add(offset);
                let buf = std::slice::from_raw_parts_mut(buf, to_read);
                let read_len = if options == InputStreamOptions::None {
                    stream.read_exact(buf).map_err(windows_core::Error::from)?;
                    to_read
                } else {
                    stream.read(buf).map_err(windows_core::Error::from)?
                };
                buffer.SetLength((offset + read_len) as u32)?;
            }
            Ok(buffer)
        }))
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

pub(super) fn transform_stream(stream: BoxedStream) -> io::Result<IHttpContent> {
    let content =
        HttpStreamContent::CreateFromInputStream(&IInputStream::from(BlockingReadInputStream {
            stream: Mutex::new(Some(stream)),
        }))?
        .cast()?;
    Ok(content)
}
