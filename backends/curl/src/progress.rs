use nyquest_interface::Progress;

/// Normalizes a libcurl total byte count to Nyquest semantics.
///
/// libcurl reports `0` when the total is unknown. Map non-positive values to
/// `None`, positive values to `Some`.
pub fn normalize_total(total: i64) -> Option<u64> {
    if total > 0 {
        Some(total as u64)
    } else {
        None
    }
}

/// Normalizes a libcurl transferred byte count.
pub fn normalize_bytes(now: i64) -> u64 {
    now.max(0) as u64
}

/// Builds a complete [`Progress`] snapshot from libcurl `XFERINFOFUNCTION`
/// parameters.
///
/// | libcurl | Nyquest |
/// |---|---|
/// | `ulnow` | `uploaded` |
/// | `ultotal` | `upload_total` |
/// | `dlnow` | `downloaded` |
/// | `dltotal` | `download_total` |
pub fn make_progress(dltotal: i64, dlnow: i64, ultotal: i64, ulnow: i64) -> Progress {
    Progress {
        uploaded: normalize_bytes(ulnow),
        upload_total: normalize_total(ultotal),
        downloaded: normalize_bytes(dlnow),
        download_total: normalize_total(dltotal),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_totals_map_to_none() {
        let p = make_progress(0, 100, 0, 50);
        assert_eq!(p.downloaded, 100);
        assert_eq!(p.uploaded, 50);
        assert_eq!(p.download_total, None);
        assert_eq!(p.upload_total, None);
    }

    #[test]
    fn known_totals() {
        let p = make_progress(1000, 250, 500, 100);
        assert_eq!(
            p,
            Progress {
                uploaded: 100,
                upload_total: Some(500),
                downloaded: 250,
                download_total: Some(1000),
            }
        );
    }
}
