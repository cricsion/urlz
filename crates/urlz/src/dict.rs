//! TLD and host dictionaries.
//!
//! Static dictionaries used to compress common TLDs and host names.
//! Entry order is normative — index is the code of record.
//! Do not reorder.

/// Identifier of this dictionary set, encoded in the 4-bit header field.
pub const DICT_SET_ID: u8 = 1;

/// TLD index meaning "no TLD" (localhost, IP addresses, bare hostnames).
pub const TLD_ESCAPE: u8 = 31;

/// Host index meaning "escape to literal host encoding" (host_mode 1 or 2).
pub const HOST_ESCAPE: u8 = 255;

/// Known TLDs; index 31 is the EMPTY sentinel.
pub const KNOWN_TLDS: [&str; 32] = [
    "com", "org", "net", "edu", "gov", "io", "co", "uk", "de", "fr", "jp", "ru", "ca", "au", "us",
    "xyz", "in", "cn", "br", "nl", "it", "es", "se", "pl", "mx", "info", "app", "dev", "ai", "me",
    "tv", "",
];

/// Common host names.
pub const COMMON_HOSTS: [&str; 40] = [
    "google",
    "youtube",
    "facebook",
    "twitter",
    "instagram",
    "linkedin",
    "reddit",
    "wikipedia",
    "amazon",
    "github",
    "stackoverflow",
    "medium",
    "netflix",
    "apple",
    "microsoft",
    "whatsapp",
    "telegram",
    "discord",
    "twitch",
    "spotify",
    "dropbox",
    "drive",
    "maps",
    "mail",
    "news",
    "play",
    "shop",
    "support",
    "docs",
    "cloud",
    "office",
    "outlook",
    "yahoo",
    "bing",
    "ebay",
    "pinterest",
    "tumblr",
    "flickr",
    "vimeo",
    "paypal",
];

/// Common path tokens for high-efficiency segment encoding.
pub const COMMON_PATH_TOKENS: [&str; 64] = [
    "api",
    "v1",
    "v2",
    "v3",
    "users",
    "user",
    "posts",
    "post",
    "comments",
    "items",
    "item",
    "products",
    "product",
    "category",
    "categories",
    "tag",
    "tags",
    "docs",
    "doc",
    "guide",
    "blog",
    "news",
    "article",
    "articles",
    "download",
    "downloads",
    "release",
    "releases",
    "assets",
    "static",
    "images",
    "img",
    "auth",
    "login",
    "signup",
    "checkout",
    "cart",
    "search",
    "watch",
    "channel",
    "status",
    "commit",
    "pull",
    "wiki",
    "questions",
    "profile",
    "settings",
    "edit",
    "view",
    "details",
    "feed",
    "app",
    "file",
    "files",
    "r",
    "u",
    "p",
    "track",
    "store",
    "hc",
    "en",
    "de",
    "fr",
    "es",
];

/// Common query parameter keys.
pub const COMMON_QUERY_KEYS: [&str; 64] = [
    "utm_source",
    "utm_medium",
    "utm_campaign",
    "utm_content",
    "utm_term",
    "q",
    "query",
    "search",
    "s",
    "page",
    "p",
    "per_page",
    "limit",
    "offset",
    "sort",
    "order",
    "filter",
    "filters",
    "category",
    "tag",
    "tags",
    "lang",
    "locale",
    "theme",
    "view",
    "tab",
    "ref",
    "source",
    "gclid",
    "fbclid",
    "id",
    "v",
    "t",
    "format",
    "type",
    "mode",
    "status",
    "callback",
    "redirect",
    "token",
    "session",
    "key",
    "state",
    "code",
    "user",
    "author",
    "date",
    "start",
    "end",
    "size",
    "color",
    "price",
    "price_min",
    "price_max",
    "min",
    "max",
    "country",
    "city",
    "lat",
    "lon",
    "zoom",
    "width",
    "height",
    "quality",
];

/// Common query parameter values.
pub const COMMON_QUERY_VALUES: [&str; 32] = [
    "true", "false", "1", "0", "desc", "asc", "newest", "all", "json", "xml", "cpc", "social",
    "email", "feed", "twitter", "facebook", "google", "en", "US", "full", "active", "dark",
    "light", "mobile", "desktop", "web", "rss", "read", "write", "admin", "public", "private",
];

/// Look up a path token in [`COMMON_PATH_TOKENS`].
#[inline]
pub fn lookup_path_token(token: &str) -> Option<u8> {
    COMMON_PATH_TOKENS
        .iter()
        .position(|&entry| entry == token)
        .map(|i| i as u8)
}

/// Return the path token at `index`.
#[inline]
pub fn path_token_at(index: u8) -> &'static str {
    assert!(
        (index as usize) < COMMON_PATH_TOKENS.len(),
        "path token index out of range: {index}"
    );
    COMMON_PATH_TOKENS[index as usize]
}

/// Look up a query key in [`COMMON_QUERY_KEYS`].
#[inline]
pub fn lookup_query_key(key: &str) -> Option<u8> {
    COMMON_QUERY_KEYS
        .iter()
        .position(|&entry| entry == key)
        .map(|i| i as u8)
}

/// Return the query key at `index`.
#[inline]
pub fn query_key_at(index: u8) -> &'static str {
    assert!(
        (index as usize) < COMMON_QUERY_KEYS.len(),
        "query key index out of range: {index}"
    );
    COMMON_QUERY_KEYS[index as usize]
}

/// Look up a query value in [`COMMON_QUERY_VALUES`].
#[inline]
pub fn lookup_query_value(val: &str) -> Option<u8> {
    COMMON_QUERY_VALUES
        .iter()
        .position(|&entry| entry == val)
        .map(|i| i as u8)
}

/// Return the query value at `index`.
#[inline]
pub fn query_value_at(index: u8) -> &'static str {
    assert!(
        (index as usize) < COMMON_QUERY_VALUES.len(),
        "query value index out of range: {index}"
    );
    COMMON_QUERY_VALUES[index as usize]
}

/// Look up a TLD in [`KNOWN_TLDS`].
///
/// Returns `Some(index)` for an exact match. Returns `None` for the
/// empty string and for unknown TLDs.
#[inline]
pub fn lookup_tld(tld: &str) -> Option<u8> {
    if tld.is_empty() {
        return None;
    }
    KNOWN_TLDS[..31]
        .iter()
        .position(|&entry| entry == tld)
        .map(|i| i as u8)
}

/// Look up a host name in [`COMMON_HOSTS`].
///
/// Returns `Some(index)` for an exact match, `None` if unknown.
#[inline]
pub fn lookup_host(host: &str) -> Option<u8> {
    COMMON_HOSTS
        .iter()
        .position(|&entry| entry == host)
        .map(|i| i as u8)
}

/// Return the TLD at `index`.
///
/// Index 31 is the EMPTY sentinel and returns `""`.
/// Panics if `index > 31`.
#[inline]
pub fn tld_at(index: u8) -> &'static str {
    assert!(index <= 31, "tld index out of range: {index}");
    KNOWN_TLDS[index as usize]
}

/// Return the host name at `index`.
///
/// Panics for any out-of-range index.
#[inline]
pub fn host_at(index: u8) -> &'static str {
    assert!(
        (index as usize) < COMMON_HOSTS.len(),
        "host index out of range: {index}"
    );
    COMMON_HOSTS[index as usize]
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn dictionaries_have_no_duplicates() {
        let dicts: &[(&str, &[&str])] = &[
            ("TLDs", &KNOWN_TLDS),
            ("Hosts", &COMMON_HOSTS),
            ("Path Tokens", &COMMON_PATH_TOKENS),
            ("Query Keys", &COMMON_QUERY_KEYS),
            ("Query Values", &COMMON_QUERY_VALUES),
        ];
        for (name, entries) in dicts {
            let mut seen = HashSet::new();
            for &entry in *entries {
                assert!(seen.insert(entry), "duplicate in {name}: {entry}");
            }
        }
    }

    #[test]
    fn dictionary_lookups_and_roundtrips() {
        for (i, entry) in KNOWN_TLDS.iter().enumerate() {
            if !entry.is_empty() {
                assert_eq!(lookup_tld(entry), Some(i as u8));
                assert_eq!(tld_at(i as u8), *entry);
            }
        }
        for (i, entry) in COMMON_HOSTS.iter().enumerate() {
            assert_eq!(lookup_host(entry), Some(i as u8));
            assert_eq!(host_at(i as u8), *entry);
        }
        for (i, entry) in COMMON_PATH_TOKENS.iter().enumerate() {
            assert_eq!(lookup_path_token(entry), Some(i as u8));
            assert_eq!(path_token_at(i as u8), *entry);
        }
        for (i, entry) in COMMON_QUERY_KEYS.iter().enumerate() {
            assert_eq!(lookup_query_key(entry), Some(i as u8));
            assert_eq!(query_key_at(i as u8), *entry);
        }
        for (i, entry) in COMMON_QUERY_VALUES.iter().enumerate() {
            assert_eq!(lookup_query_value(entry), Some(i as u8));
            assert_eq!(query_value_at(i as u8), *entry);
        }
    }

    #[test]
    fn ordering_snapshot() {
        assert_eq!(tld_at(0), "com");
        assert_eq!(tld_at(30), "tv");
        assert_eq!(host_at(0), "google");
        assert_eq!(host_at(39), "paypal");
    }

    #[test]
    fn escape_and_edge_behavior() {
        assert_eq!(lookup_tld(""), None);
        assert_eq!(lookup_tld("example"), None);
        assert_eq!(lookup_host("notasite"), None);
        assert_eq!(tld_at(31), "");
        assert_eq!(lookup_path_token("nonexistent"), None);
        assert_eq!(lookup_query_key("nonexistent"), None);
        assert_eq!(lookup_query_value("nonexistent"), None);
    }
}
