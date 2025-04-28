use nyquest_interface::Result as NyquestResult;
use windows::Storage::Streams::IBuffer;
use windows::Win32::System::WinRT::IBufferByteAccess;
use windows_core::Interface;

use crate::error::IntoNyquestResult;

pub(crate) trait IBufferExt {
    fn to_vec(&self) -> NyquestResult<Vec<u8>>;
}

impl IBufferExt for IBuffer {
    fn to_vec(&self) -> NyquestResult<Vec<u8>> {
        let len = self.Length().into_nyquest_result()? as usize;
        let iba = self.cast::<IBufferByteAccess>().into_nyquest_result()?;
        let arr = unsafe {
            let ptr = iba.Buffer().into_nyquest_result()?;
            std::slice::from_raw_parts(ptr, len).to_vec()
        };
        Ok(arr)
    }
}
