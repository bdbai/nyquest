//! URL parsing utilities for WinHTTP backend.

use windows_sys::Win32::Networking::WinHttp::*;
use windows_sys::Win32::UI::Shell::*;

/// Parsed URL components.
#[allow(dead_code)]
pub(crate) struct ParsedUrl {
    pub host: String,
    pub port: u16,
    pub path: String,
    pub is_secure: bool,
}

impl ParsedUrl {
    /// Parses a URL string into its components using WinHttpCrackUrl.
    pub fn parse(url: &str) -> Option<Self> {
        // Convert to wide string
        let url_wide: Vec<u16> = url.encode_utf16().chain(std::iter::once(0)).collect();

        // Allocate a single buffer for all URL components
        // Layout: [scheme: 32][host: 256][path: 2048][extra: 1024]
        const SCHEME_SIZE: usize = 32;
        const HOST_SIZE: usize = 256;
        const PATH_SIZE: usize = 2048;
        const EXTRA_SIZE: usize = 1024;
        const TOTAL_SIZE: usize = SCHEME_SIZE + HOST_SIZE + PATH_SIZE + EXTRA_SIZE;

        let mut buffer = vec![0u16; TOTAL_SIZE];
        let (scheme_buffer, remaining_buffer) = buffer.split_at_mut(SCHEME_SIZE);
        let (host_buffer, remaining_buffer) = remaining_buffer.split_at_mut(HOST_SIZE);
        let (path_buffer, extra_buffer) = remaining_buffer.split_at_mut(PATH_SIZE);

        let mut components;
        let result;
        unsafe {
            components = URL_COMPONENTS {
                dwStructSize: std::mem::size_of::<URL_COMPONENTS>() as u32,
                lpszScheme: scheme_buffer.as_mut_ptr(),
                dwSchemeLength: SCHEME_SIZE as u32,
                nScheme: 0,
                lpszHostName: host_buffer.as_mut_ptr(),
                dwHostNameLength: HOST_SIZE as u32,
                nPort: 0,
                lpszUserName: std::ptr::null_mut(),
                dwUserNameLength: 0,
                lpszPassword: std::ptr::null_mut(),
                dwPasswordLength: 0,
                lpszUrlPath: path_buffer.as_mut_ptr(),
                dwUrlPathLength: PATH_SIZE as u32,
                lpszExtraInfo: extra_buffer.as_mut_ptr(),
                dwExtraInfoLength: EXTRA_SIZE as u32,
            };

            result = WinHttpCrackUrl(
                url_wide.as_ptr(),
                url_wide.len() as u32 - 1,
                0,
                &mut components,
            )
        }

        if result == 0 {
            return None;
        }

        // Extract host from buffer
        let host_len = host_buffer
            .iter()
            .position(|&c| c == 0)
            .unwrap_or(HOST_SIZE);
        let host_str = String::from_utf16_lossy(&host_buffer[..host_len]);

        // Extract path from buffer
        let path_len = path_buffer
            .iter()
            .position(|&c| c == 0)
            .unwrap_or(PATH_SIZE);
        let mut path_str = String::from_utf16_lossy(&path_buffer[..path_len]);

        // Append extra info (query string, fragment) if present
        let extra_len = extra_buffer
            .iter()
            .position(|&c| c == 0)
            .unwrap_or(EXTRA_SIZE);
        if extra_len > 0 {
            path_str.push_str(&String::from_utf16_lossy(&extra_buffer[..extra_len]));
        }

        // Default to "/" if path is empty
        if path_str.is_empty() {
            path_str = "/".to_string();
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

        Some(ParsedUrl {
            host: host_str,
            port,
            path: path_str,
            is_secure,
        })
    }
}

/// Concatenates base URL with relative URI using UrlCombineW.
#[allow(dead_code)]
pub(crate) fn concat_url(base_url: Option<&str>, relative_uri: &str) -> String {
    match base_url {
        Some(base) => {
            // If relative_uri is already absolute, return it as-is
            if relative_uri.starts_with("http://") || relative_uri.starts_with("https://") {
                return relative_uri.to_string();
            }

            // Convert strings to wide
            let base_wide: Vec<u16> = base.encode_utf16().chain(std::iter::once(0)).collect();
            let relative_wide: Vec<u16> = relative_uri
                .encode_utf16()
                .chain(std::iter::once(0))
                .collect();

            // Allocate buffer for combined URL
            let mut buffer = vec![0u16; 2048];
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
                String::from_utf16_lossy(&buffer[..len])
            } else {
                // On error, return the relative URI as-is
                relative_uri.to_string()
            }
        }
        None => relative_uri.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_http_url() {
        let url = ParsedUrl::parse("http://example.com/path").unwrap();
        assert_eq!(url.host, "example.com");
        assert_eq!(url.port, 80);
        assert_eq!(url.path, "/path");
        assert!(!url.is_secure);
    }

    #[test]
    fn test_parse_https_url_with_port() {
        let url = ParsedUrl::parse("https://example.com:8443/api/v1").unwrap();
        assert_eq!(url.host, "example.com");
        assert_eq!(url.port, 8443);
        assert_eq!(url.path, "/api/v1");
        assert!(url.is_secure);
    }

    #[test]
    fn test_parse_url_no_path() {
        let url = ParsedUrl::parse("https://example.com").unwrap();
        assert_eq!(url.host, "example.com");
        assert_eq!(url.port, 443);
        assert_eq!(url.path, "/");
        assert!(url.is_secure);
    }

    #[test]
    fn test_concat_url() {
        assert_eq!(
            concat_url(Some("https://api.example.com"), "/users"),
            "https://api.example.com/users"
        );
        assert_eq!(
            concat_url(Some("https://api.example.com/aa"), "users"),
            "https://api.example.com/users"
        );
        assert_eq!(
            concat_url(None, "https://example.com/path"),
            "https://example.com/path"
        );
    }
}
