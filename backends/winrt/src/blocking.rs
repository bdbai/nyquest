use std::io;

use nyquest_interface::blocking::Request;
use nyquest_interface::blocking::{BlockingBackend, BlockingClient, BlockingResponse};
use nyquest_interface::client::{BuildClientResult, ClientOptions};
use nyquest_interface::{Error as NyquestError, Result as NyquestResult};
use windows::core::{Interface, HSTRING};
use windows::Foundation::Uri;
use windows::Web::Http::{HttpClient, HttpCompletionOption};
use windows::Win32::System::WinRT::IBufferByteAccess;

use crate::client::WinrtClientExt;
use crate::error::IntoNyquestResult;
use crate::request::{create_body, create_request};
use crate::response::WinrtResponse;
use crate::uri::build_uri;

#[derive(Clone)]
pub struct WinrtBlockingClient {
    base_url: Option<HSTRING>,
    client: HttpClient,
}

impl crate::WinrtBackend {
    pub fn create_blocking_client(
        &self,
        options: ClientOptions,
    ) -> io::Result<WinrtBlockingClient> {
        let base_url = options.base_url.as_ref().map(HSTRING::from);
        let client = HttpClient::create(options)?;
        Ok(WinrtBlockingClient { base_url, client })
    }
}

impl WinrtBlockingClient {
    fn send_request(&self, uri: &Uri, req: Request) -> io::Result<WinrtResponse> {
        let req_msg = create_request(uri, &req)?;
        // TODO: stream
        if let Some(body) = req.body {
            let body = create_body(body, &mut |_| unimplemented!())?;
            req_msg.SetContent(&body)?;
        }
        let res = self
            .client
            .SendRequestWithOptionAsync(&req_msg, HttpCompletionOption::ResponseHeadersRead)?
            .get()?;
        WinrtResponse::new(res)
    }
}

impl BlockingClient for WinrtBlockingClient {
    type Response = WinrtResponse;
    fn request(&self, req: Request) -> NyquestResult<Self::Response> {
        let uri =
            build_uri(&self.base_url, &req.relative_uri).map_err(|_| NyquestError::InvalidUrl)?;
        self.send_request(&uri, req).into_nyquest_result()
    }
}

impl BlockingBackend for crate::WinrtBackend {
    type BlockingClient = WinrtBlockingClient;

    fn create_blocking_client(
        &self,
        options: ClientOptions,
    ) -> BuildClientResult<Self::BlockingClient> {
        Ok(self.create_blocking_client(options).into_nyquest_result()?)
    }
}

impl BlockingResponse for WinrtResponse {
    fn status(&self) -> u16 {
        self.status
    }

    fn get_header(&self, header: &str) -> nyquest_interface::Result<Vec<String>> {
        self.get_header(header).into_nyquest_result()
    }

    fn content_length(&self) -> Option<u64> {
        self.content_length
    }

    fn text(&mut self) -> NyquestResult<String> {
        let content = self
            .content()
            .into_nyquest_result()?
            .ReadAsStringAsync()
            .into_nyquest_result()?
            .get()
            .into_nyquest_result()?;
        Ok(content.to_string_lossy())
    }

    fn bytes(&mut self) -> NyquestResult<Vec<u8>> {
        let content = self
            .content()
            .into_nyquest_result()?
            .ReadAsBufferAsync()
            .into_nyquest_result()?
            .get()
            .into_nyquest_result()?;
        let iba = content.cast::<IBufferByteAccess>().into_nyquest_result()?;
        let arr = unsafe {
            let len = content.Length().into_nyquest_result()? as usize;
            let ptr = iba.Buffer().into_nyquest_result()?;
            let mut arr = Vec::with_capacity(len);
            std::ptr::copy_nonoverlapping(ptr, arr.as_mut_ptr(), len);
            arr.set_len(len);
            arr
        };
        Ok(arr)
    }
}

impl io::Read for WinrtResponse {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let reader = self.reader_mut()?;

        let mut size = reader.UnconsumedBufferLength()?;
        if size == 0 {
            let loaded = reader.LoadAsync(buf.len() as u32)?.get()?;
            if loaded == 0 {
                return Ok(0);
            }
            size = reader.UnconsumedBufferLength()?;
        }
        let size = buf.len().min(size as usize);
        let buf = &mut buf[..size];
        reader.ReadBytes(buf)?;
        Ok(size)
    }
}
