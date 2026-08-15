//! URL parsing and normalization.
//!
//! [`parse_url`] splits a URL into the semantic components consumed by the
//! urlz encoder. Parsing is deterministic and lossless: the
//! encoder reconstructs the URL from the returned [`ParsedUrl`], so every
//! field preserves the exact normalized input.
//!
//! # Normalization
//!
//! - Scheme and host are lowercased (browsers treat them case-insensitively).
//! - Non-ASCII characters are percent-encoded per browser semantics: each character is UTF-8 encoded and every byte is written as `%XX`.
//! - Existing `%XX` escapes are kept but their hex digits are normalized to uppercase (`%2f` → `%2F`). Malformed escapes (`%zz`, trailing `%`) are rejected with [`Error::InvalidUrl`].
//! - ASCII characters are preserved verbatim — structural characters (`/`, `?`, `#`, `&`, `=`) are not double-encoded, and no `+`→space decoding is applied.
//! - Path, query, and fragment segments keep their raw (percent-encoded) form.
//!
//! # Decisions
//!
//! - **Scheme:** only `http`/`https` are accepted; any other scheme or a missing scheme is an error.
//! - **Userinfo** (`user:pass@host`): rejected with a clear error for deterministic behavior.
//! - **IPv6 literals** (`[::1]`): rejected with a clear error for v1.
//! - **Ports:** default ports (80/443) are dropped silently. A non-default port is kept as a literal suffix on the host string.
//! - **Host/TLD split:** split at the last dot. IPv4 literals are exempt and keep the whole string in `host` with an empty `tld`.
//! - **Path segments:** the leading `/` is structural and produces no segment; a trailing `/` produces a final empty segment.
//! - **Index suffix:** the last path segment stays in `path_segments` for round-trip fidelity; `index_suffix` is a compression hint.

use std::fmt;
use std::str::FromStr;

use crate::error::Error;

/// The semantic components of a parsed URL.
///
/// The encoder reconstructs the URL from these fields, so they preserve the
/// exact normalized input (see the module docs for the normalization rules).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ParsedUrl {
    /// `true` for `https://`, `false` for `http://`.
    pub https: bool,
    /// `true` if the host had a leading `www.` label (stripped from [`Self::host`]).
    pub www: bool,
    /// Host name excluding the TLD (e.g. `example` from `example.com`).
    ///
    /// For IP literals and dot-less hosts (e.g. `localhost`) this is the whole
    /// host. A non-default port is kept as a literal suffix here or in
    /// [`Self::tld`] (see module docs).
    pub host: String,
    /// TLD only (e.g. `com` from `example.com`); empty for localhost/IP/bare
    /// hostnames.
    pub tld: String,
    /// Compression hint for a trailing index-file path segment.
    pub index_suffix: IndexSuffix,
    /// Path segments (leading `/` is structural, trailing `/` yields a final
    /// empty segment).
    pub path_segments: Vec<String>,
    /// Query key/value pairs in order. A pair without `=` has a `None` value.
    pub query_segments: Vec<(String, Option<String>)>,
    /// Fragment segments split on `/`.
    pub fragment_segments: Vec<String>,
}

impl Default for ParsedUrl {
    fn default() -> Self {
        Self {
            https: true,
            www: false,
            host: String::new(),
            tld: String::new(),
            index_suffix: IndexSuffix::None,
            path_segments: Vec::new(),
            query_segments: Vec::new(),
            fragment_segments: Vec::new(),
        }
    }
}

impl fmt::Display for ParsedUrl {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let scheme = if self.https { "https" } else { "http" };
        let www = if self.www { "www." } else { "" };
        let host = if self.tld.is_empty() {
            format!("{www}{}", self.host)
        } else {
            format!("{www}{}.{}", self.host, self.tld)
        };
        let path = if self.path_segments.is_empty() {
            String::new()
        } else {
            format!("/{}", self.path_segments.join("/"))
        };
        let query = if self.query_segments.is_empty() {
            String::new()
        } else {
            let pairs = self
                .query_segments
                .iter()
                .map(|(k, v)| match v {
                    Some(v) => format!("{k}={v}"),
                    None => k.clone(),
                })
                .collect::<Vec<_>>()
                .join("&");
            format!("?{pairs}")
        };
        let fragment = if self.fragment_segments.is_empty() {
            String::new()
        } else {
            format!("#{}", self.fragment_segments.join("/"))
        };
        write!(f, "{scheme}://{host}{path}{query}{fragment}")
    }
}

impl FromStr for ParsedUrl {
    type Err = Error;

    #[inline]
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        parse_url(s)
    }
}

impl TryFrom<&str> for ParsedUrl {
    type Error = Error;

    #[inline]
    fn try_from(s: &str) -> Result<Self, Self::Error> {
        parse_url(s)
    }
}

/// Classification of a trailing index-file path segment.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
pub enum IndexSuffix {
    /// No index suffix (or no path).
    #[default]
    None,
    /// The last path segment is exactly `index.html`.
    IndexHtml,
    /// The last path segment is exactly `index.php`.
    IndexPhp,
    /// The last path segment is another `index.*` file; the literal text is
    /// preserved for round-trip fidelity.
    Other(String),
}

/// Parse a URL into its semantic components.
///
/// See the module docs for the exact normalization and edge-case rules.
pub fn parse_url(s: &str) -> Result<ParsedUrl, Error> {
    let normalized = normalize_percent_encoding(s)?;

    let scheme_end = normalized.find("://").ok_or_else(|| Error::InvalidUrl {
        reason: "missing scheme (expected http:// or https://)".to_string(),
    })?;
    let scheme = &normalized[..scheme_end];
    if scheme.is_empty() {
        return Err(Error::InvalidUrl {
            reason: "missing scheme".to_string(),
        });
    }
    let https = match scheme.to_ascii_lowercase().as_str() {
        "http" => false,
        "https" => true,
        other => {
            return Err(Error::InvalidUrl {
                reason: format!("unsupported scheme: {other} (only http and https are supported)"),
            });
        }
    };
    let rest = &normalized[scheme_end + 3..];

    let authority_end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let authority = &rest[..authority_end];
    if authority.is_empty() {
        return Err(Error::InvalidUrl {
            reason: "empty host".to_string(),
        });
    }
    if authority.contains('@') {
        return Err(Error::InvalidUrl {
            reason: "userinfo (user:pass@host) is not supported".to_string(),
        });
    }
    if authority.starts_with('[') {
        return Err(Error::InvalidUrl {
            reason: "IPv6 literals are not supported in v1".to_string(),
        });
    }

    // Split host[:port] at the last ':' (IPv6 is rejected above, so there is
    // at most one ':').
    let (host_part, port) = match authority.rsplit_once(':') {
        Some((h, p)) => {
            if p.is_empty() {
                return Err(Error::InvalidUrl {
                    reason: "empty port".to_string(),
                });
            }
            let port: u16 = p.parse().map_err(|_| Error::InvalidUrl {
                reason: format!("invalid port: {p}"),
            })?;
            (h, Some(port))
        }
        None => (authority, None),
    };
    if host_part.contains(':') {
        return Err(Error::InvalidUrl {
            reason: "malformed authority: multiple colons".to_string(),
        });
    }

    let port = match port {
        Some(p) if (https && p == 443) || (!https && p == 80) => None,
        other => other,
    };
    let port_suffix = port.map_or_else(String::new, |p| format!(":{p}"));

    // Strip a leading "www." label (case-insensitive) and lowercase the host.
    // `to_ascii_lowercase` also lowercases percent-escape hex digits, so
    // re-uppercase them to match the normalization used everywhere else.
    let lower_host = uppercase_escape_hex(&host_part.to_ascii_lowercase());
    let (host_label, www) = match lower_host.strip_prefix("www.") {
        Some(rest) if !rest.is_empty() => (rest.to_string(), true),
        Some(_) => {
            return Err(Error::InvalidUrl {
                reason: "empty host after stripping www.".to_string(),
            });
        }
        None => (lower_host, false),
    };
    if host_label.is_empty() {
        return Err(Error::InvalidUrl {
            reason: "empty host".to_string(),
        });
    }

    // Host/TLD split at the last dot; IPv4 literals are exempt.
    let (host, tld) = if is_ipv4_literal(&host_label) {
        (format!("{host_label}{port_suffix}"), String::new())
    } else {
        let full_host = format!("{host_label}{port_suffix}");
        match full_host.rsplit_once('.') {
            Some((h, t)) => (h.to_string(), t.to_string()),
            None => (full_host, String::new()),
        }
    };
    if host.is_empty() {
        return Err(Error::InvalidUrl {
            reason: "empty host".to_string(),
        });
    }

    let resource = &rest[authority_end..];
    let (path_query, fragment) = match resource.split_once('#') {
        Some((pq, f)) => (pq, f),
        None => (resource, ""),
    };
    let (path, query, query_present) = match path_query.split_once('?') {
        Some((p, q)) => (p, q, true),
        None => (path_query, "", false),
    };

    let path_segments = split_path(path);
    let query_segments = split_query(query, query_present);
    let fragment_segments = split_fragment(fragment);
    let index_suffix = detect_index_suffix(&path_segments);

    Ok(ParsedUrl {
        https,
        www,
        host,
        tld,
        index_suffix,
        path_segments,
        query_segments,
        fragment_segments,
    })
}

/// Percent-encode non-ASCII characters and normalize existing `%XX` escapes.
///
/// Non-ASCII characters are UTF-8 encoded and each byte is written as `%XX`
/// (non-ASCII bytes are never RFC 3986 unreserved). ASCII characters are
/// preserved verbatim. Existing escapes keep their hex digits uppercased.
/// Malformed escapes (`%zz`, trailing `%`) are rejected.
fn normalize_percent_encoding(input: &str) -> Result<String, Error> {
    let mut out = String::with_capacity(input.len());
    let mut chars = input.chars();
    while let Some(c) = chars.next() {
        if c == '%' {
            match (chars.next(), chars.next()) {
                (Some(hi), Some(lo)) if hi.is_ascii_hexdigit() && lo.is_ascii_hexdigit() => {
                    out.push('%');
                    out.push(hi.to_ascii_uppercase());
                    out.push(lo.to_ascii_uppercase());
                }
                _ => {
                    return Err(Error::InvalidUrl {
                        reason: "invalid percent-escape in URL".to_string(),
                    });
                }
            }
        } else if c.is_ascii() {
            out.push(c);
        } else {
            let mut buf = [0u8; 4];
            for &byte in c.encode_utf8(&mut buf).as_bytes() {
                push_percent_encoded(&mut out, byte);
            }
        }
    }
    Ok(out)
}

/// Append `%XX` (uppercase hex) for `byte` to `out`.
fn push_percent_encoded(out: &mut String, byte: u8) {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    out.push('%');
    out.push(HEX[(byte >> 4) as usize] as char);
    out.push(HEX[(byte & 0x0F) as usize] as char);
}

/// Re-uppercase `%XX` hex digits after ASCII lowercasing. The input was
/// validated by [`normalize_percent_encoding`], so every `%` is followed by
/// two hex digits.
fn uppercase_escape_hex(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == '%' {
            out.push('%');
            for _ in 0..2 {
                match chars.next() {
                    Some(h) if h.is_ascii_hexdigit() => out.push(h.to_ascii_uppercase()),
                    Some(h) => out.push(h),
                    None => break,
                }
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// Split a path into segments.
///
/// The leading `/` is structural and produces no segment; a trailing `/`
/// produces a final empty segment; consecutive interior slashes each produce
/// one empty segment. `""` → `[]`, `"/"` → `[""]`, `"//"` → `["", ""]`.
fn split_path(path: &str) -> Vec<String> {
    if path.is_empty() {
        return Vec::new();
    }
    let rest = path.strip_prefix('/').unwrap_or(path);
    rest.split('/').map(str::to_string).collect()
}

/// Split a query into key/value pairs.
///
/// Pairs are split on `&`, then each pair on the first `=`. A pair without
/// `=` has a `None` value. A present-but-empty query (`?`) yields one empty
/// key. An absent query yields no pairs.
fn split_query(query: &str, present: bool) -> Vec<(String, Option<String>)> {
    if !present {
        return Vec::new();
    }
    query
        .split('&')
        .map(|pair| match pair.split_once('=') {
            Some((k, v)) => (k.to_string(), Some(v.to_string())),
            None => (pair.to_string(), None),
        })
        .collect()
}

/// Split a fragment into segments on `/` (no structural leading slash).
fn split_fragment(fragment: &str) -> Vec<String> {
    if fragment.is_empty() {
        return Vec::new();
    }
    fragment.split('/').map(str::to_string).collect()
}

/// Classify the last path segment as an index suffix, if any.
///
/// Exact `index.html`/`index.php` map to the compact codes; any other
/// `index.*` (case-insensitive prefix) maps to [`IndexSuffix::Other`] with the
/// literal text. The segment itself stays in `path_segments` for round-trip
/// fidelity.
fn detect_index_suffix(path_segments: &[String]) -> IndexSuffix {
    let Some(last) = path_segments.last() else {
        return IndexSuffix::None;
    };
    if last == "index.html" {
        IndexSuffix::IndexHtml
    } else if last == "index.php" {
        IndexSuffix::IndexPhp
    } else if last.to_ascii_lowercase().starts_with("index.") {
        IndexSuffix::Other(last.clone())
    } else {
        IndexSuffix::None
    }
}

/// `true` if `s` is an IPv4 literal: four dot-separated groups of 1–3 ASCII
/// digits, each ≤ 255.
fn is_ipv4_literal(s: &str) -> bool {
    let mut count = 0;
    for p in s.split('.') {
        if p.is_empty()
            || p.len() > 3
            || !p.bytes().all(|b| b.is_ascii_digit())
            || p.parse::<u8>().is_err()
        {
            return false;
        }
        count += 1;
    }
    count == 4
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn parse(s: &str) -> ParsedUrl {
        parse_url(s).unwrap_or_else(|e| panic!("parse_url({s:?}) failed: {e}"))
    }

    #[test]
    fn parsed_url_traits() {
        let p = parse("https://example.com/path");
        assert_eq!(p.to_string(), "https://example.com/path");
        let parsed_again: ParsedUrl = "https://example.com/path".parse().unwrap();
        assert_eq!(p, parsed_again);

        let d = ParsedUrl::default();
        assert!(d.https);
    }

    #[test]
    fn url_components_table() {
        struct Case<'a> {
            url: &'a str,
            https: bool,
            www: bool,
            host: &'a str,
            tld: &'a str,
            path_segments: &'a [&'a str],
        }

        let cases = [
            Case {
                url: "http://example.com/",
                https: false,
                www: false,
                host: "example",
                tld: "com",
                path_segments: &[""],
            },
            Case {
                url: "HTTP://EXAMPLE.COM",
                https: false,
                www: false,
                host: "example",
                tld: "com",
                path_segments: &[],
            },
            Case {
                url: "https://www.example.com/",
                https: true,
                www: true,
                host: "example",
                tld: "com",
                path_segments: &[""],
            },
            Case {
                url: "https://WWW.Example.COM/",
                https: true,
                www: true,
                host: "example",
                tld: "com",
                path_segments: &[""],
            },
            Case {
                url: "https://www/",
                https: true,
                www: false,
                host: "www",
                tld: "",
                path_segments: &[""],
            },
            Case {
                url: "http://localhost/",
                https: false,
                www: false,
                host: "localhost",
                tld: "",
                path_segments: &[""],
            },
            Case {
                url: "http://127.0.0.1/",
                https: false,
                www: false,
                host: "127.0.0.1",
                tld: "",
                path_segments: &[""],
            },
            Case {
                url: "http://example.com:80/",
                https: false,
                www: false,
                host: "example",
                tld: "com",
                path_segments: &[""],
            },
            Case {
                url: "https://example.com:443/",
                https: true,
                www: false,
                host: "example",
                tld: "com",
                path_segments: &[""],
            },
            Case {
                url: "http://example.com:8080/",
                https: false,
                www: false,
                host: "example",
                tld: "com:8080",
                path_segments: &[""],
            },
            Case {
                url: "http://localhost:8080/",
                https: false,
                www: false,
                host: "localhost:8080",
                tld: "",
                path_segments: &[""],
            },
            Case {
                url: "https://example.com/a-b_c.d~e",
                https: true,
                www: false,
                host: "example",
                tld: "com",
                path_segments: &["a-b_c.d~e"],
            },
            Case {
                url: "https://example.com/a/b/",
                https: true,
                www: false,
                host: "example",
                tld: "com",
                path_segments: &["a", "b", ""],
            },
            Case {
                url: "https://example.com//",
                https: true,
                www: false,
                host: "example",
                tld: "com",
                path_segments: &["", ""],
            },
            Case {
                url: "https://example.com///",
                https: true,
                www: false,
                host: "example",
                tld: "com",
                path_segments: &["", "", ""],
            },
            Case {
                url: "https://example.com/a//b/",
                https: true,
                www: false,
                host: "example",
                tld: "com",
                path_segments: &["a", "", "b", ""],
            },
        ];

        for c in cases {
            let p = parse(c.url);
            assert_eq!(p.https, c.https, "https mismatch for {}", c.url);
            assert_eq!(p.www, c.www, "www mismatch for {}", c.url);
            assert_eq!(p.host, c.host, "host mismatch for {}", c.url);
            assert_eq!(p.tld, c.tld, "tld mismatch for {}", c.url);
            assert_eq!(
                p.path_segments, c.path_segments,
                "path mismatch for {}",
                c.url
            );
        }
    }

    #[test]
    fn invalid_urls_rejected() {
        let invalid_cases = [
            // Invalid ports
            "http://example.com:abc/",
            "http://example.com:/",
            "http://example.com:99999/",
            // Userinfo
            "http://user:pass@example.com/",
            "http://user@example.com/",
            // IPv6
            "http://[::1]/",
            "http://[2001:db8::1]:8080/",
            // Invalid schemes
            "ftp://example.com/",
            "file:///etc/passwd",
            "javascript:alert(1)",
            "data:text/plain,hi",
            "mailto:x@y.com",
            // Missing scheme
            "example.com/path",
            "//example.com/path",
            // Empty host
            "http:///path",
            "http://?x=1",
            "http://www.",
            "http://:8080/",
            // Invalid percent escapes
            "https://example.com/%zz",
            "https://example.com/%",
            "https://example.com/%2",
            "https://example.com/%2g",
        ];

        for s in invalid_cases {
            assert!(
                matches!(parse_url(s), Err(Error::InvalidUrl { .. })),
                "should reject {s}"
            );
        }
    }

    #[test]
    fn percent_encoding_and_normalization() {
        let p = parse("https://example.com/日本語?q=かえで");
        assert_eq!(p.path_segments, ["%E6%97%A5%E6%9C%AC%E8%AA%9E"]);
        assert_eq!(
            p.query_segments,
            [(
                "q".to_string(),
                Some("%E3%81%8B%E3%81%88%E3%81%A7".to_string())
            )]
        );

        let p1 = parse("https://ex%C3%A4mple.com/");
        let p2 = parse("https://exämple.com/");
        assert_eq!(p1.host, "ex%C3%A4mple");
        assert_eq!(p2.host, "ex%C3%A4mple");

        let p3 = parse("https://example.com/a%2fb%2Fc");
        assert_eq!(p3.path_segments, ["a%2Fb%2Fc"]);
    }

    #[test]
    fn query_and_fragment_parsing() {
        let p = parse("https://example.com/path");
        assert!(p.query_segments.is_empty());

        let p = parse("https://example.com/path?");
        assert_eq!(p.query_segments, [("".to_string(), None)]);

        let p = parse("https://example.com/?a&b=2");
        assert_eq!(
            p.query_segments,
            [
                ("a".to_string(), None),
                ("b".to_string(), Some("2".to_string())),
            ]
        );

        let p = parse("https://example.com/?a=1=2");
        assert_eq!(
            p.query_segments,
            [("a".to_string(), Some("1=2".to_string()))]
        );

        let p = parse("https://example.com/?a=1&");
        assert_eq!(
            p.query_segments,
            [
                ("a".to_string(), Some("1".to_string())),
                ("".to_string(), None),
            ]
        );

        let p = parse("https://example.com/#a/b");
        assert_eq!(p.fragment_segments, ["a", "b"]);

        let p = parse("https://example.com/#");
        assert!(p.fragment_segments.is_empty());

        let p = parse("https://example.com/#/a");
        assert_eq!(p.fragment_segments, ["", "a"]);
    }

    #[test]
    fn index_suffix_classification() {
        let cases = [
            (
                "https://example.com/index.html",
                IndexSuffix::IndexHtml,
                vec!["index.html"],
            ),
            (
                "https://example.com/a/index.php",
                IndexSuffix::IndexPhp,
                vec!["a", "index.php"],
            ),
            (
                "https://example.com/index.aspx",
                IndexSuffix::Other("index.aspx".to_string()),
                vec!["index.aspx"],
            ),
            (
                "https://example.com/INDEX.HTML",
                IndexSuffix::Other("INDEX.HTML".to_string()),
                vec!["INDEX.HTML"],
            ),
            (
                "https://example.com/about.html",
                IndexSuffix::None,
                vec!["about.html"],
            ),
        ];

        for (url, expected_suffix, expected_path) in cases {
            let p = parse(url);
            assert_eq!(p.index_suffix, expected_suffix, "suffix mismatch for {url}");
            assert_eq!(p.path_segments, expected_path, "path mismatch for {url}");
        }
    }

    #[test]
    fn very_long_path() {
        let segs: Vec<String> = (0..100).map(|i| format!("seg{i}")).collect();
        let url = format!("https://example.com/{}", segs.join("/"));
        let p = parse(&url);
        assert_eq!(p.path_segments.len(), 100);
        assert_eq!(p.path_segments, segs);
    }

    #[test]
    fn full_url_round_trip_components() {
        let p = parse("https://www.example.com:8443/a/b?x=1&y#frag");
        assert!(p.https);
        assert!(p.www);
        assert_eq!(p.host, "example");
        assert_eq!(p.tld, "com:8443");
        assert_eq!(p.path_segments, ["a", "b"]);
        assert_eq!(
            p.query_segments,
            [
                ("x".to_string(), Some("1".to_string())),
                ("y".to_string(), None),
            ]
        );
        assert_eq!(p.fragment_segments, ["frag"]);
    }

    #[test]
    fn parse_reconstruct_round_trip() {
        let urls = [
            "https://example.com/path",
            "http://example.com/",
            "https://www.example.com/a/b/?x=1&y#frag",
            "http://localhost:8080/",
            "http://127.0.0.1/",
            "https://example.com/index.html",
            "https://example.com/日本語?q=かえで",
            "https://example.com//",
            "https://example.com/a%2Fb",
        ];
        for url in urls {
            let p = parse_url(url).unwrap();
            let rebuilt = p.to_string();
            let p2 = parse_url(&rebuilt).unwrap();
            assert_eq!(p, p2, "round-trip failed for {url} (rebuilt: {rebuilt})");
        }
    }

    fn url_strategy() -> impl Strategy<Value = String> {
        let scheme = prop_oneof![Just("http"), Just("https")];
        let www = prop_oneof![Just("www."), Just("")];
        let label = proptest::collection::vec(proptest::char::range('a', 'z'), 1..10)
            .prop_map(|v| v.into_iter().collect::<String>());
        let tld = proptest::collection::vec(proptest::char::range('a', 'z'), 2..6)
            .prop_map(|v| v.into_iter().collect::<String>());
        let host = (www, label, tld).prop_map(|(w, l, t)| format!("{w}{l}.{t}"));
        let seg_char = prop_oneof![
            proptest::char::range('a', 'z'),
            Just('日'),
            Just('é'),
            Just('-'),
            Just('_'),
        ];
        let path_seg = proptest::collection::vec(seg_char, 0..8)
            .prop_map(|v| v.into_iter().collect::<String>());
        let path = proptest::collection::vec(path_seg, 0..4).prop_map(|segs| {
            if segs.is_empty() {
                String::new()
            } else {
                format!("/{}", segs.join("/"))
            }
        });
        let key = proptest::collection::vec(proptest::char::range('a', 'z'), 0..5)
            .prop_map(|v| v.into_iter().collect::<String>());
        let value = proptest::collection::vec(proptest::char::range('a', 'z'), 0..5)
            .prop_map(|v| v.into_iter().collect::<String>());
        let query = proptest::collection::vec((key, value), 0..4).prop_map(|pairs| {
            if pairs.is_empty() {
                String::new()
            } else {
                let joined = pairs
                    .iter()
                    .map(|(k, v)| format!("{k}={v}"))
                    .collect::<Vec<_>>()
                    .join("&");
                format!("?{joined}")
            }
        });
        let frag = proptest::collection::vec(proptest::char::range('a', 'z'), 0..5).prop_map(|v| {
            let s = v.into_iter().collect::<String>();
            if s.is_empty() {
                String::new()
            } else {
                format!("#{s}")
            }
        });
        (scheme, host, path, query, frag).prop_map(|(s, h, p, q, f)| format!("{s}://{h}{p}{q}{f}"))
    }

    proptest! {
    #[test]
    fn parse_is_deterministic(url in url_strategy()) {
    let a = parse_url(&url).unwrap();
    let b = parse_url(&url).unwrap();
    prop_assert_eq!(a, b);
    }
    }
}
