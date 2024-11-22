use windows::core as windows_core;
use windows::core::*;
use windows::Storage::Streams::{IBuffer, IBuffer_Impl};
use windows::Win32::System::WinRT::*;

#[implement(IBuffer, IBufferByteAccess)]
pub(crate) struct VecBuffer {
    data: std::cell::UnsafeCell<Vec<u8>>,
}

impl VecBuffer {
    pub fn new(data: Vec<u8>) -> Self {
        if u32::try_from(data.len()).is_err() {
            panic!("VecBuffer::new: data too large");
        }
        Self {
            data: std::cell::UnsafeCell::new(data),
        }
    }
}

#[allow(non_snake_case)]
impl IBuffer_Impl for VecBuffer_Impl {
    fn Capacity(&self) -> windows_core::Result<u32> {
        Ok(unsafe {
            let buf = &mut *self.data.get();
            buf.capacity() as u32
        })
    }

    fn Length(&self) -> windows_core::Result<u32> {
        Ok(unsafe {
            let buf = &mut *self.data.get();
            buf.len() as u32
        })
    }

    fn SetLength(&self, value: u32) -> windows_core::Result<()> {
        unsafe {
            let buf = &mut *self.data.get();
            buf.resize(value as usize, 0);
        }
        Ok(())
    }
}

#[allow(non_snake_case)]
impl IBufferByteAccess_Impl for VecBuffer_Impl {
    fn Buffer(&self) -> Result<*mut u8> {
        unsafe { Ok((*self.data.get()).as_mut_ptr()) }
    }
}
