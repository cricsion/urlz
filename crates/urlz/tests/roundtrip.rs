//! Cross-layer integration tests: encode/decode round-trips over the fixture
//! corpus, version/dict-set rejection, and randomized property-based URL round-trips.

use proptest::prelude::*;

use urlz::alphabet::{BASE85_ALPHABET, biguint_from_bytes_be, to_base};
use urlz::decode::decode;
use urlz::encode::{encode, encode_to_bits};
use urlz::error::Error;

const CORPUS_PATH: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/corpus.txt");

fn corpus_entries() -> Vec<String> {
    let text = std::fs::read_to_string(CORPUS_PATH).expect("fixture corpus must exist");
    text.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(str::to_string)
        .collect()
}

fn chars_in_alphabet(s: &str, alphabet: &[u8]) -> bool {
    s.bytes().all(|b| alphabet.contains(&b))
}

#[test]
fn corpus_roundtrips_exactly_in_base85() {
    let entries = corpus_entries();
    assert!(!entries.is_empty(), "corpus must not be empty");
    for url in &entries {
        let payload = encode(url).unwrap_or_else(|e| panic!("encode({url:?}) failed: {e}"));
        assert!(
            chars_in_alphabet(&payload, BASE85_ALPHABET),
            "payload for {url:?} contains chars outside BASE85_ALPHABET: {payload:?}"
        );
        let decoded = decode(&payload).unwrap_or_else(|e| panic!("decode({url:?}) failed: {e}"));
        assert_eq!(decoded, *url, "round-trip mismatch for {url:?}");
    }
}

#[test]
fn rejects_invalid_header_fields() {
    let url = "https://example.com/path";
    let valid_bits = encode_to_bits(url).expect("encode_to_bits must succeed");

    // Version is the first nibble (top 4 bits of byte 0, MSB-first).
    let mut bad_version = valid_bits.clone();
    bad_version[0] = (bad_version[0] & 0x0F) | (3 << 4);
    let n = biguint_from_bytes_be(&bad_version);
    let payload = to_base(&n, BASE85_ALPHABET);
    match decode(&payload) {
        Err(Error::UnsupportedVersion(3)) => {}
        other => panic!("expected UnsupportedVersion(3), got {other:?}"),
    }

    // dict_set_id is the second nibble (bits 4..=7 of byte 0).
    let mut bad_dict = valid_bits;
    bad_dict[0] = (bad_dict[0] & 0xF0) | 2;
    let n = biguint_from_bytes_be(&bad_dict);
    let payload = to_base(&n, BASE85_ALPHABET);
    assert!(
        decode(&payload).is_err(),
        "expected error for corrupted dict_set_id"
    );
}

/// Build normalized URLs from components, matching urlparse's reconstruction.
///
/// Constrained by urlparse.rs normalization rules: scheme/host are lowercase,
/// `www.` is a separate flag, host/tld split at the last dot, path segments
/// keep empty segments, query pairs split on the first `=`, and a fragment is
/// a single segment (an empty fragment would be dropped by the parser).
fn url_strategy() -> impl Strategy<Value = String> {
    let scheme = prop_oneof![Just("http"), Just("https")];
    let www = prop_oneof![Just("www."), Just("")];
    let host = prop_oneof![
        Just("example"),
        Just("github"),
        Just("google"),
        Just("localhost"),
        Just("example-site"),
        Just("mysite"),
        Just("sub.domain"),
    ];
    let tld = prop_oneof![
        Just("com".to_string()),
        Just("org".to_string()),
        Just("net".to_string()),
        Just("io".to_string()),
        Just("co".to_string()),
        Just("uk".to_string()),
        Just("de".to_string()),
        Just("ai".to_string()),
        Just("app".to_string()),
        Just("dev".to_string()),
        proptest::collection::vec(proptest::char::range('a', 'z'), 1..=5)
            .prop_map(|v| v.into_iter().collect::<String>()),
        Just(String::new()),
    ];
    let seg_char = prop_oneof![
        proptest::char::range('a', 'z'),
        proptest::char::range('0', '9'),
        Just('-'),
        Just('.'),
        Just('_'),
        Just('~'),
        Just('!'),
        Just('*'),
        Just('('),
        Just(')'),
        Just(','),
        Just(';'),
        Just(':'),
        Just('@'),
        Just('$'),
    ];
    let segment = proptest::collection::vec(seg_char.clone(), 0..=8)
        .prop_map(|v| v.into_iter().collect::<String>());
    let path = proptest::collection::vec(segment, 0..=8).prop_map(|segs| {
        if segs.is_empty() {
            String::new()
        } else {
            format!("/{}", segs.join("/"))
        }
    });
    let q_char = prop_oneof![
        proptest::char::range('a', 'z'),
        proptest::char::range('0', '9'),
        Just('-'),
        Just('.'),
        Just('_'),
        Just('~'),
        Just('!'),
        Just('*'),
        Just('('),
        Just(')'),
        Just(','),
        Just(';'),
        Just(':'),
        Just('@'),
        Just('$'),
        Just('+'),
        Just('='),
    ];
    let q_key = proptest::collection::vec(q_char.clone(), 0..=6)
        .prop_map(|v| v.into_iter().collect::<String>());
    let q_value = prop_oneof![
        Just(None),
        Just(Some(String::new())),
        proptest::collection::vec(q_char, 0..=6)
            .prop_map(|v| Some(v.into_iter().collect::<String>())),
    ];
    let query = proptest::collection::vec((q_key, q_value), 0..=3).prop_map(|pairs| {
        if pairs.is_empty() {
            String::new()
        } else {
            let joined = pairs
                .iter()
                .map(|(k, v)| match v {
                    Some(v) => format!("{k}={v}"),
                    None => k.clone(),
                })
                .collect::<Vec<_>>()
                .join("&");
            format!("?{joined}")
        }
    });
    let fragment = proptest::collection::vec(seg_char, 0..=8).prop_map(|v| {
        let s = v.into_iter().collect::<String>();
        if s.is_empty() {
            String::new()
        } else {
            format!("#{s}")
        }
    });
    (scheme, www, host, tld, path, query, fragment).prop_map(|(s, w, h, t, p, q, f)| {
        let host_part = if t.is_empty() {
            format!("{w}{h}")
        } else {
            format!("{w}{h}.{t}")
        };
        format!("{s}://{host_part}{p}{q}{f}")
    })
}

proptest! {
 #![proptest_config(ProptestConfig {
 cases: 256,
 ..ProptestConfig::default()
 })]

 #[test]
 fn generated_urls_roundtrip_exactly(url in url_strategy()) {
 let encoded = encode(&url).unwrap_or_else(|e| panic!("encode({url:?}) failed: {e}"));
 let decoded = decode(&encoded).unwrap_or_else(|e| panic!("decode({url:?}) failed: {e}"));
 prop_assert_eq!(decoded, url);
 }
}
