fn is_absolute(url: &str) -> bool {
    url.len() >= 8
        && (url[..7].eq_ignore_ascii_case("http://") || url[..8].eq_ignore_ascii_case("https://"))
}

pub(crate) fn concat_url(base: Option<&str>, relative: &str) -> String {
    let Some(base) = base.filter(|_| !is_absolute(relative)) else {
        return relative.into();
    };
    let (proto, protsep) = base.split_once("//").unwrap_or(("", base));
    let host_path = protsep.split_once('?').unwrap_or((protsep, "")).0;
    if relative.starts_with("//") {
        proto.to_owned() + relative
    } else if relative.starts_with('/') {
        let host = host_path
            .split_once('/')
            .map_or(host_path, |(host, _)| host);
        format!("{proto}//{host}{relative}")
    } else {
        let pathsep = host_path
            .rsplit_once('/')
            .map_or(host_path, |(pathsep, _)| pathsep);
        format!("{proto}//{pathsep}/{relative}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_absolute() {
        let urls = [
            "http://example.com",
            "https://example.com",
            "HTTP://EXAMPLE.COM",
            "HTTPS://EXAMPLE.COM",
        ];
        for url in urls {
            assert!(is_absolute(url), "{url}");
        }
    }

    #[test]
    fn test_is_not_absolute() {
        let urls = ["example.com", "/example", "example"];
        for url in urls {
            assert!(!is_absolute(url), "{url}");
        }
    }

    #[test]
    fn test_concat_url() {
        let testcases = [
            (None, "http://example.com", "http://example.com"),
            (Some("http://a.com"), "https://b.com", "https://b.com"),
            (Some("http://a.com"), "//b.com", "http://b.com"),
            (Some("https://a.com"), "//b.com", "https://b.com"),
            (Some("http://a.com"), "/b", "http://a.com/b"),
            (Some("http://a.com/a/b"), "/c", "http://a.com/c"),
            (Some("http://a.com?q=1"), "/c", "http://a.com/c"),
            (Some("http://a.com/a/b"), "c", "http://a.com/a/c"),
            (Some("http://a.com/a/b/"), "c", "http://a.com/a/b/c"),
            (Some("http://a.com/a/b?q=1"), "c", "http://a.com/a/c"),
            (Some("http://a.com?q=1"), "c", "http://a.com/c"),
        ];
        for (base, relative, expected) in testcases {
            assert_eq!(
                concat_url(base, relative),
                expected,
                "{}, {relative}, {expected}",
                base.unwrap_or("None"),
            );
        }
    }
}
