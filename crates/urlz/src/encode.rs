//! urlz URL encoder.
//!
//! [`encode_to_bits`] serializes a parsed URL into the canonical urlz bitstream;
//! [`encode`] converts it into a compact base85 payload. The decoder lives in
//! [`crate::decode::decode`].
//!
//! # Bit Layout
//!
//! - **Header** (12 bits):
//!   - version (4 bits): [`WIRE_VERSION`] (2)
//!   - dict_set_id (4 bits): [`DICT_SET_ID`] (1)
//!   - https (1 bit): 1 = https, 0 = http
//!   - www (1 bit): 1 = `www.` prefix present
//!   - index_suffix code (2 bits): 0 = None, 1 = index.html, 2 = index.php, 3 = Other
//! - **Port**:
//!   - port_flag (1 bit): 0 = default (dropped), 1 = non-default port
//!   - if port_flag: mode (2 bits): 0 = 8080, 1 = 8443, 2 = 3000, 3 = raw u16 (16 bits)
//! - **Host**:
//!   - subdomain_flag (1 bit):
//!     - 1 = multi-label host: count - 2 (3 bits, for 2..=9 labels), followed by each label:
//!       - is_dict (1 bit): 1 = dict index (8 bits), 0 = segment
//!     - 0 = single host: mode (2 bits): 0 = dict index (8 bits), 1 = base26-lower, 2 = literal
//! - **TLD**:
//!   - is_known (1 bit): 0 = known TLD index (5 bits), 1 = literal segment
//! - **Index Suffix Literal**: present only if index_suffix code is 3 (one segment)
//! - **Resource Presence Flags** (3 bits): path_present (1), query_present (1), fragment_present (1)
//! - **Path** (if present):
//!   - segment count (compact 4-bit nibble / varint)
//!   - for each segment: is_dict (1 bit): 1 = token index (7 bits), 0 = segment
//! - **Query** (if present):
//!   - pair count (compact 4-bit nibble / varint)
//!   - for each pair:
//!     - key: is_dict (1 bit): 1 = query key index (6 bits), 0 = segment
//!     - value: has_value (1 bit): if 1, is_dict (1 bit): 1 = query val index (5 bits), 0 = segment
//! - **Fragment** (if present):
//!   - segment count (compact 4-bit nibble / varint), then segments
//!
//! ## Segment Layout
//! - Pure `%XX` hex bytes: alphabet 7 (3 bits), count, then 8-bit raw bytes
//! - Huffman compressed: alphabet 5 (3 bits), count, stream-encoded prefix codes
//! - Raw fallback: alphabet 6 (3 bits), count, 8-bit raw bytes
//! - Radix base-N: alphabet_id (3 bits), count, fixed-width zero-padded integer bits

use num_bigint::BigUint;

use crate::alphabet::{BASE85_ALPHABET, biguint_from_bytes_be, to_base};
use crate::bitstream::WriteBitStream;
use crate::decode::{MAX_PAYLOAD_BYTES, MAX_SEGMENT_COUNT, MAX_SYMBOL_COUNT, MAX_VALUE_BIT_LENGTH};
use crate::dict::{
    DICT_SET_ID, TLD_ESCAPE, lookup_host, lookup_path_token, lookup_query_key, lookup_query_value,
    lookup_tld,
};
use crate::error::Error;
use crate::huffman::DEFAULT_HUFFMAN_ENCODER;
use crate::segment::{
    analyze_segment, decode_pure_percent, fixed_bit_len, is_pure_percent, value_to_biguint,
};
use crate::urlparse::{IndexSuffix, parse_url};

/// The canonical wire format version.
pub const WIRE_VERSION: u8 = 2;

/// Encode segment count in 4 bits for counts 0..14; write 15 then varint for larger counts.
#[inline]
pub(crate) fn write_compact_count(bs: &mut WriteBitStream, count: usize) -> Result<(), Error> {
    if count < 15 {
        bs.write_bits(count as u64, 4)
    } else {
        bs.write_bits(15, 4)?;
        bs.write_varint((count - 15) as u64)
    }
}

/// Write exactly `bit_len` bits of `value` zero-padded on the left to `bit_len`.
pub(crate) fn write_biguint_fixed(
    bs: &mut WriteBitStream,
    value: &BigUint,
    bit_len: u32,
) -> Result<(), Error> {
    if bit_len == 0 {
        return Ok(());
    }
    let val_bits = value.bits() as u32;
    if val_bits > bit_len {
        return Err(Error::InvalidPayload {
            reason: format!("value requires {val_bits} bits, exceeding capacity {bit_len}"),
        });
    }
    let mut zeros = bit_len - val_bits;
    while zeros > 0 {
        let chunk = zeros.min(64);
        bs.write_bits(0, chunk)?;
        zeros -= chunk;
    }
    if val_bits > 0 {
        let bytes = value.to_bytes_be();
        let leading_bits = val_bits % 8;
        let mut offset = 0;
        if leading_bits != 0 {
            let mask = (1u8 << leading_bits) - 1;
            bs.write_bits((bytes[0] & mask) as u64, leading_bits)?;
            offset = 1;
        }
        for chunk in bytes[offset..].chunks(8) {
            let mut v = 0u64;
            for &byte in chunk {
                v = (v << 8) | u64::from(byte);
            }
            bs.write_bits(v, (chunk.len() * 8) as u32)?;
        }
    }
    Ok(())
}

/// Serialize a single URL segment into the bitstream.
pub(crate) fn write_segment(
    bs: &mut WriteBitStream,
    s: &str,
    huffman_allowed: bool,
) -> Result<(), Error> {
    if is_pure_percent(s) {
        let raw = decode_pure_percent(s);
        if raw.len() > MAX_SYMBOL_COUNT as usize {
            return Err(Error::InvalidUrl {
                reason: format!("segment has {} symbols (max {MAX_SYMBOL_COUNT})", raw.len()),
            });
        }
        bs.write_bits(7, 3)?; // alphabet_id = 7 (PctBytes)
        write_compact_count(bs, raw.len())?;
        for &b in &raw {
            bs.write_bits(b as u64, 8)?;
        }
        return Ok(());
    }

    let seg = analyze_segment(s);
    if seg.symbol_count > MAX_SYMBOL_COUNT as usize {
        return Err(Error::InvalidUrl {
            reason: format!(
                "segment has {} symbols (max {MAX_SYMBOL_COUNT})",
                seg.symbol_count
            ),
        });
    }

    if huffman_allowed
        && seg.symbol_count > 0
        && let Ok(huff_len) = DEFAULT_HUFFMAN_ENCODER.bit_len(&seg.value)
    {
        let base_bit_len = fixed_bit_len(seg.alphabet_id, seg.symbol_count);
        if (huff_len as u32) < base_bit_len && (huff_len as u64) <= MAX_VALUE_BIT_LENGTH {
            bs.write_bits(5, 3)?; // alphabet_id = 5 (Huffman)
            write_compact_count(bs, seg.symbol_count)?;
            let written = DEFAULT_HUFFMAN_ENCODER.encode_into(bs, &seg.value)?;
            debug_assert_eq!(written, huff_len);
            return Ok(());
        }
    }

    if seg.alphabet_id == 6 {
        bs.write_bits(6, 3)?;
        write_compact_count(bs, seg.symbol_count)?;
        for &b in &seg.value {
            bs.write_bits(b as u64, 8)?;
        }
        return Ok(());
    }

    let value = value_to_biguint(&seg)?;
    let bit_len = fixed_bit_len(seg.alphabet_id, seg.symbol_count);
    bs.write_bits(seg.alphabet_id as u64, 3)?;
    write_compact_count(bs, seg.symbol_count)?;
    write_biguint_fixed(bs, &value, bit_len)?;
    Ok(())
}

/// Encode a URL to the canonical raw bitstream bytes.
pub fn encode_to_bits(url: &str) -> Result<Vec<u8>, Error> {
    let parsed = parse_url(url)?;
    let mut bs = WriteBitStream::new();

    // --- Header (12 bits) ---
    bs.write_bits(WIRE_VERSION as u64, 4)?;
    bs.write_bits(DICT_SET_ID as u64, 4)?;
    bs.write_bits(parsed.https as u64, 1)?;
    bs.write_bits(parsed.www as u64, 1)?;
    let index_code: u64 = match &parsed.index_suffix {
        IndexSuffix::None => 0,
        IndexSuffix::IndexHtml => 1,
        IndexSuffix::IndexPhp => 2,
        IndexSuffix::Other(_) => 3,
    };
    bs.write_bits(index_code, 2)?;

    // --- Extract Port ---
    let (clean_tld, port_from_tld) = match parsed.tld.split_once(':') {
        Some((t, p)) => (t, p.parse::<u16>().ok()),
        None => (parsed.tld.as_str(), None),
    };
    let (clean_host, port_from_host) = match parsed.host.split_once(':') {
        Some((h, p)) => (h, p.parse::<u16>().ok()),
        None => (parsed.host.as_str(), None),
    };
    let port = port_from_tld.or(port_from_host);

    // Port flag & encoding
    bs.write_bits(port.is_some() as u64, 1)?;
    if let Some(p) = port {
        match p {
            8080 => bs.write_bits(0, 2)?,
            8443 => bs.write_bits(1, 2)?,
            3000 => bs.write_bits(2, 2)?,
            other => {
                bs.write_bits(3, 2)?;
                bs.write_bits(other as u64, 16)?;
            }
        }
    }

    // --- Host ---
    let host_labels: Vec<&str> = clean_host.split('.').collect();
    if host_labels.len() > 1 && host_labels.len() <= 9 {
        bs.write_bits(1, 1)?; // subdomain_flag = 1
        bs.write_bits((host_labels.len() - 2) as u64, 3)?;
        for label in host_labels {
            if let Some(idx) = lookup_host(label) {
                bs.write_bits(1, 1)?; // is_dict = 1
                bs.write_bits(idx as u64, 8)?;
            } else {
                bs.write_bits(0, 1)?; // is_dict = 0
                write_segment(&mut bs, label, false)?;
            }
        }
    } else {
        bs.write_bits(0, 1)?; // subdomain_flag = 0
        if let Some(idx) = lookup_host(clean_host) {
            bs.write_bits(0, 2)?; // mode 0: dict
            bs.write_bits(idx as u64, 8)?;
        } else if clean_host.bytes().all(|b| b.is_ascii_lowercase()) {
            bs.write_bits(1, 2)?; // mode 1: base26-lower
            write_segment(&mut bs, clean_host, false)?;
        } else {
            bs.write_bits(2, 2)?; // mode 2: mixed-case / literal
            write_segment(&mut bs, clean_host, false)?;
        }
    }

    // --- TLD ---
    if clean_tld.is_empty() {
        bs.write_bits(0, 1)?;
        bs.write_bits(TLD_ESCAPE as u64, 5)?;
    } else if let Some(idx) = lookup_tld(clean_tld) {
        bs.write_bits(0, 1)?;
        bs.write_bits(idx as u64, 5)?;
    } else {
        bs.write_bits(1, 1)?;
        write_segment(&mut bs, clean_tld, false)?;
    }

    // --- Index-suffix literal (only for Other(_)) ---
    if let IndexSuffix::Other(literal) = &parsed.index_suffix {
        write_segment(&mut bs, literal, true)?;
    }

    // --- Resource Regions ---
    let path_present = !parsed.path_segments.is_empty();
    let query_present = !parsed.query_segments.is_empty();
    let fragment_present = !parsed.fragment_segments.is_empty();
    bs.write_bits(path_present as u64, 1)?;
    bs.write_bits(query_present as u64, 1)?;
    bs.write_bits(fragment_present as u64, 1)?;

    if path_present {
        let segments = if parsed.index_suffix != IndexSuffix::None {
            &parsed.path_segments[..parsed.path_segments.len() - 1]
        } else {
            &parsed.path_segments[..]
        };
        if segments.len() > MAX_SEGMENT_COUNT as usize {
            return Err(Error::InvalidUrl {
                reason: format!(
                    "path has {} segments (max {MAX_SEGMENT_COUNT})",
                    segments.len()
                ),
            });
        }
        write_compact_count(&mut bs, segments.len())?;
        for s in segments {
            if let Some(token_idx) = lookup_path_token(s) {
                bs.write_bits(1, 1)?;
                bs.write_bits(token_idx as u64, 7)?;
            } else {
                bs.write_bits(0, 1)?;
                write_segment(&mut bs, s, true)?;
            }
        }
    }

    if query_present {
        if parsed.query_segments.len() > MAX_SEGMENT_COUNT as usize {
            return Err(Error::InvalidUrl {
                reason: format!(
                    "query has {} segments (max {MAX_SEGMENT_COUNT})",
                    parsed.query_segments.len()
                ),
            });
        }
        write_compact_count(&mut bs, parsed.query_segments.len())?;
        for (key, value) in &parsed.query_segments {
            // Key encoding
            if let Some(key_idx) = lookup_query_key(key) {
                bs.write_bits(1, 1)?;
                bs.write_bits(key_idx as u64, 6)?;
            } else {
                bs.write_bits(0, 1)?;
                write_segment(&mut bs, key, true)?;
            }

            // Value encoding
            match value {
                Some(v) => {
                    bs.write_bits(1, 1)?; // has_value = 1
                    if let Some(val_idx) = lookup_query_value(v) {
                        bs.write_bits(1, 1)?;
                        bs.write_bits(val_idx as u64, 5)?;
                    } else {
                        bs.write_bits(0, 1)?;
                        write_segment(&mut bs, v, true)?;
                    }
                }
                None => {
                    bs.write_bits(0, 1)?; // has_value = 0
                }
            }
        }
    }

    if fragment_present {
        if parsed.fragment_segments.len() > MAX_SEGMENT_COUNT as usize {
            return Err(Error::InvalidUrl {
                reason: format!(
                    "fragment has {} segments (max {MAX_SEGMENT_COUNT})",
                    parsed.fragment_segments.len()
                ),
            });
        }
        write_compact_count(&mut bs, parsed.fragment_segments.len())?;
        for s in &parsed.fragment_segments {
            write_segment(&mut bs, s, true)?;
        }
    }

    Ok(bs.into_bytes())
}

/// Encode a URL into a compact base85 payload.
///
/// # Examples
///
/// ```
/// let payload = urlz::encode("https://github.com/rust-lang/rust")?;
/// assert!(!payload.is_empty());
/// assert!(payload
///     .chars()
///     .all(|c| urlz::alphabet::BASE85_ALPHABET.contains(&(c as u8))));
/// # Ok::<(), urlz::Error>(())
/// ```
pub fn encode(url: &str) -> Result<String, Error> {
    let bits = encode_to_bits(url)?;
    let n = biguint_from_bytes_be(&bits);
    let payload = to_base(&n, BASE85_ALPHABET);
    if payload.len() > MAX_PAYLOAD_BYTES {
        return Err(Error::InvalidUrl {
            reason: format!(
                "encoded payload is {} bytes (max {MAX_PAYLOAD_BYTES})",
                payload.len()
            ),
        });
    }
    Ok(payload)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::decode::decode;
    use crate::dict::{COMMON_HOSTS, KNOWN_TLDS};

    /// Assert that `decode(encode(input))` produces `expected`.
    fn roundtrip(input: &str, expected: &str) {
        let encoded = encode(input).unwrap_or_else(|e| panic!("encode({input:?}) failed: {e}"));
        let decoded = decode(&encoded).unwrap_or_else(|e| panic!("decode({input:?}) failed: {e}"));
        assert_eq!(decoded, expected, "round-trip mismatch for {input:?}");
    }

    /// Assert that an already-canonical URL roundtrips identically to itself.
    fn roundtrip_canonical(u: &str) {
        roundtrip(u, u);
    }

    #[test]
    fn roundtrip_representative_urls() {
        // TLDs and hosts
        for tld in &KNOWN_TLDS[..31] {
            roundtrip_canonical(&format!("https://example.{tld}/"));
        }
        for host in &COMMON_HOSTS {
            roundtrip_canonical(&format!("https://{host}.com/"));
        }
        let canonical_urls = [
            "http://localhost",
            "https://127.0.0.1",
            "http://192.168.1.1/path",
            "https://example-site.com/x",
            "https://sub.domain.co.uk",
            "https://example.com:8080/path",
            "http://localhost:3000/api",
            "https://www.example.com/",
            "https://example.com/index.html",
            "https://example.com/a/index.php",
            "https://example.com/index.aspx",
            "https://example.com/",
            "https://example.com/a/b/",
            "https://example.com",
            "https://example.com/?a=1&b",
            "https://example.com/?a=1=2&b&c=",
            "https://example.com/?q=",
            "https://example.com/?",
            "https://example.com/path#section",
            "https://example.com/#top",
            "https://example.com/a,b(c)d!e*f;g",
            "https://example.com/path",
            "https://example.com/a!b",
        ];
        for url in canonical_urls {
            roundtrip_canonical(url);
        }

        // Explicit normalization test cases
        roundtrip("https://MySite.COM", "https://mysite.com");
        roundtrip("https://WWW.Example.com/", "https://www.example.com/");
        roundtrip(
            "https://example.com/日本語?q=かえで",
            "https://example.com/%E6%97%A5%E6%9C%AC%E8%AA%9E?q=%E3%81%8B%E3%81%88%E3%81%A7",
        );

        // Long query roundtrip
        let q = "x".repeat(1000);
        roundtrip_canonical(&format!("https://example.com/?q={q}"));
    }

    #[test]
    fn default_port_is_dropped() {
        let encoded = encode("https://example.com:443/").unwrap();
        assert_eq!(decode(&encoded).unwrap(), "https://example.com/");
    }

    #[test]
    fn write_segment_huffman_decisions() {
        // "test" wins: Huffman selected (alphabet_id = 5)
        let mut bs = WriteBitStream::new();
        write_segment(&mut bs, "test", true).unwrap();
        let bytes = bs.into_bytes();
        let mut r = crate::bitstream::ReadBitStream::from_bytes(&bytes);
        assert_eq!(r.read_bits(3).unwrap(), 5);
        assert_eq!(r.read_bits(4).unwrap(), 4); // count 4

        // "x" and "a" stay base26 (alphabet_id = 1)
        for s in ["x", "a"] {
            let mut bs = WriteBitStream::new();
            write_segment(&mut bs, s, true).unwrap();
            let bytes = bs.into_bytes();
            let mut r = crate::bitstream::ReadBitStream::from_bytes(&bytes);
            assert_eq!(
                r.read_bits(3).unwrap(),
                1,
                "segment {s:?} should stay base26"
            );
        }

        // Host literals are pinned without Huffman
        let mut bs = WriteBitStream::new();
        write_segment(&mut bs, "path", false).unwrap();
        let bytes = bs.into_bytes();
        let mut r = crate::bitstream::ReadBitStream::from_bytes(&bytes);
        assert_eq!(r.read_bits(3).unwrap(), 1);

        // Raw segment "a&b" wins with Huffman (alphabet_id = 5)
        let mut bs = WriteBitStream::new();
        write_segment(&mut bs, "a&b", true).unwrap();
        let bytes = bs.into_bytes();
        let mut r = crate::bitstream::ReadBitStream::from_bytes(&bytes);
        assert_eq!(r.read_bits(3).unwrap(), 5);
    }

    #[test]
    fn segment_and_payload_limits() {
        // Path segment count limits (max 64)
        let path_64 = (0..64)
            .map(|i| format!("s{i}"))
            .collect::<Vec<_>>()
            .join("/");
        roundtrip_canonical(&format!("https://example.com/{path_64}"));

        let path_65 = (0..65)
            .map(|i| format!("s{i}"))
            .collect::<Vec<_>>()
            .join("/");
        assert!(matches!(
            encode(&format!("https://example.com/{path_65}")),
            Err(Error::InvalidUrl { .. })
        ));

        // Symbol count limits (max 4096)
        roundtrip_canonical(&format!("https://example.com/{}", "x".repeat(4096)));
        assert!(matches!(
            encode(&format!("https://example.com/{}", "x".repeat(4097))),
            Err(Error::InvalidUrl { .. })
        ));

        // Total payload length limit (> 65536 bytes)
        let seg = "x".repeat(2048);
        let url = format!(
            "https://example.com/{}",
            std::iter::repeat_n(seg.as_str(), 48)
                .collect::<Vec<_>>()
                .join("/")
        );
        assert!(matches!(encode(&url), Err(Error::InvalidUrl { .. })));
    }
}
