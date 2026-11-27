//! NSURLSession progress tracking helpers.
//!
//! Maps `URLSessionTask` byte-count properties to Nyquest [`Progress`]:
//!
//! | NSURLSessionTask | Nyquest |
//! |---|---|
//! | `countOfBytesSent` | `uploaded` |
//! | `countOfBytesExpectedToSend` | `upload_total` |
//! | `countOfBytesReceived` | `downloaded` |
//! | `countOfBytesExpectedToReceive` | `download_total` |
//!
//! `NSURLSessionTransferSizeUnknown` (-1) identifies unknown totals and maps
//! to `None`.

use nyquest_interface::Progress;

/// Value of `NSURLSessionTransferSizeUnknown`.
pub const TRANSFER_SIZE_UNKNOWN: i64 = -1;

/// Normalizes an expected byte count: `NSURLSessionTransferSizeUnknown`
/// (and any negative value) maps to `None`.
pub fn normalize_transfer_size(expected: i64) -> Option<u64> {
    if expected < 0 {
        None
    } else {
        Some(expected as u64)
    }
}

/// Normalizes a transferred byte count.
pub fn normalize_bytes(count: i64) -> u64 {
    count.max(0) as u64
}

/// Builds a complete [`Progress`] snapshot from `URLSessionTask` counters.
pub fn make_progress(
    bytes_sent: i64,
    bytes_expected_to_send: i64,
    bytes_received: i64,
    bytes_expected_to_receive: i64,
) -> Progress {
    Progress {
        uploaded: normalize_bytes(bytes_sent),
        upload_total: normalize_transfer_size(bytes_expected_to_send),
        downloaded: normalize_bytes(bytes_received),
        download_total: normalize_transfer_size(bytes_expected_to_receive),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_totals() {
        let p = make_progress(10, TRANSFER_SIZE_UNKNOWN, 20, TRANSFER_SIZE_UNKNOWN);
        assert_eq!(p.uploaded, 10);
        assert_eq!(p.upload_total, None);
        assert_eq!(p.downloaded, 20);
        assert_eq!(p.download_total, None);
    }

    #[test]
    fn known_totals() {
        let p = make_progress(5, 100, 10, 200);
        assert_eq!(
            p,
            Progress {
                uploaded: 5,
                upload_total: Some(100),
                downloaded: 10,
                download_total: Some(200),
            }
        );
    }

    #[test]
    fn zero_known() {
        let p = make_progress(0, 0, 0, 0);
        assert_eq!(p.upload_total, Some(0));
        assert_eq!(p.download_total, Some(0));
    }
}
