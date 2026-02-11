//! URL parsing utilities for WinHTTP backend.

use std::ptr::null_mut;
use std::slice;

use windows_sys::Win32::Networking::WinHttp::*;
use windows_sys::Win32::UI::Shell::*;

/// Parsed URL components.
pub(crate) struct ParsedUrl<'b> {
    pub host: &'b [u16],
    pub port: u16,
    pub path: &'b [u16],
    pub is_secure: bool,
}

impl<'b> ParsedUrl<'b> {
    /// Parses a URL string into its components using WinHttpCrackUrl.
    pub fn parse(url_wide: &'b [u16]) -> Option<Self> {
        let mut components;
        let result;
        unsafe {
            components = URL_COMPONENTS {
                dwStructSize: std::mem::size_of::<URL_COMPONENTS>() as u32,
                lpszScheme: null_mut(),
                dwSchemeLength: u32::MAX,
                nScheme: 0,
                lpszHostName: null_mut(),
                dwHostNameLength: u32::MAX,
                nPort: 0,
                lpszUserName: null_mut(),
                dwUserNameLength: 0,
                lpszPassword: null_mut(),
                dwPasswordLength: 0,
                lpszUrlPath: null_mut(),
                dwUrlPathLength: u32::MAX,
                lpszExtraInfo: null_mut(),
                dwExtraInfoLength: 0,
            };

            result = WinHttpCrackUrl(url_wide.as_ptr(), url_wide.len() as u32, 0, &mut components)
        }

        if result == 0 {
            return None;
        }

        // Determine if secure based on scheme
        // WinHttpCrackUrl uses INTERNET_SCHEME_* constants from WinInet
        // HTTP = 1, HTTPS = 2 (not WinHTTP's constants!)
        let is_secure = components.nScheme == 2; // INTERNET_SCHEME_HTTPS from WinInet

        // Use the port from the structure, or default based on scheme
        let port = if components.nPort == 0 {
            if is_secure {
                443
            } else {
                80
            }
        } else {
            components.nPort
        };

        unsafe {
            let host = slice::from_raw_parts(
                components.lpszHostName,
                components.dwHostNameLength as usize,
            );
            let mut path =
                slice::from_raw_parts(components.lpszUrlPath, components.dwUrlPathLength as usize);
            if !path.starts_with(&[b'/' as u16]) {
                path = &[b'/' as u16];
            }

            Some(ParsedUrl {
                host,
                port,
                path,
                is_secure,
            })
        }
    }
}

/// Concatenates base URL with relative URI using UrlCombineW.
pub(crate) fn concat_url(base_url_wide: Option<&[u16]>, relative_uri: &str) -> Vec<u16> {
    let mut relative_wide: Vec<u16> = relative_uri
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect();
    let is_absolute = relative_uri.starts_with("http://") || relative_uri.starts_with("https://");
    if let Some(base_wide) = base_url_wide.filter(|_| !is_absolute) {
        if !base_wide.ends_with(&[0]) {
            panic!("base_url must be null-terminated wide string");
        }

        // Allocate buffer for combined URL
        let estimated_len = base_wide.len() * 2 + relative_wide.len() + 2;
        let mut buffer = vec![0u16; estimated_len];
        let mut buffer_len = buffer.len() as u32;

        let result = unsafe {
            UrlCombineW(
                base_wide.as_ptr(),
                relative_wide.as_ptr(),
                buffer.as_mut_ptr(),
                &mut buffer_len,
                0, // dwFlags
            )
        };

        if result == 0 {
            // S_OK = 0, find the null terminator
            let len = buffer.iter().position(|&c| c == 0).unwrap_or(buffer.len());
            buffer.truncate(len);
            buffer.shrink_to_fit();
            return buffer;
        }
    }

    relative_wide.pop();
    relative_wide
}

#[cfg(test)]
mod tests {
    use super::*;

    use widestring::{u16cstr, u16str};

    #[test]
    fn test_parse_http_url() {
        let url = ParsedUrl::parse(u16cstr!("http://example.com/path").as_slice()).unwrap();
        assert_eq!(url.host, u16cstr!("example.com").as_slice());
        assert_eq!(url.port, 80);
        assert_eq!(url.path, u16cstr!("/path").as_slice());
        assert!(!url.is_secure);
    }

    #[test]
    fn test_parse_https_url_with_port() {
        let url = ParsedUrl::parse(u16cstr!("https://example.com:8443/api/v1").as_slice()).unwrap();
        assert_eq!(url.host, u16cstr!("example.com").as_slice());
        assert_eq!(url.port, 8443);
        assert_eq!(url.path, u16cstr!("/api/v1").as_slice());
        assert!(url.is_secure);
    }

    #[test]
    fn test_parse_url_no_path() {
        let url = ParsedUrl::parse(u16cstr!("https://example.com/?1").as_slice()).unwrap();
        assert_eq!(url.host, u16cstr!("example.com").as_slice());
        assert_eq!(url.port, 443);
        assert_eq!(url.path, u16cstr!("/?1").as_slice());
        assert!(url.is_secure);
    }

    #[test]
    fn test_concat_url() {
        assert_eq!(
            concat_url(
                Some(u16cstr!("https://api.example.com").as_slice_with_nul()),
                "/users"
            ),
            u16str!("https://api.example.com/users").as_slice()
        );
        assert_eq!(
            concat_url(
                Some(u16cstr!("https://api.example.com/aa").as_slice_with_nul()),
                "users?id=1"
            ),
            u16str!("https://api.example.com/users?id=1").as_slice()
        );
        assert_eq!(
            concat_url(None, "https://example.com/path"),
            u16str!("https://example.com/path").as_slice()
        );
    }
}
