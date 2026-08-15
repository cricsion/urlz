//! urlz URL encoder.
//!
//! [`encode_to_bits`] serializes a parsed URL into the wire format bit layout;
//! [`encode`] wraps it in the base85 wire alphabet. The decoder lives in
//! [`crate::decode::decode`].
//!
//! # Bit layout
//!
//! - **Header** (12 bits): version (4), dict_set_id (4), https (1), www (1), index_suffix code (2).
//! - **Host** (2-bit mode): 0 = dictionary index (8 bits), 1 = literal lowercase segment, 2 = literal mixed-case segment.
//! - **TLD** (1-bit mode): 0 = known index (5 bits), 1 = literal segment.
//! - **Index-suffix literal** (only when the header code is 3): one segment.
//! - **Resource**: path/query/fragment presence flags (3 bits), then for each present region a segment-count varint followed by that many segments.
//!
//! Each segment is `alphabet_id` (4 bits), `symbol_count` varint,
//! `value_bit_length` varint, then the value's bits MSB-first. For
//! `alphabet_id` 5 the value bits are huffman codes instead of a base-N integer.

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
use crate::segment::{SegmentEncoding, analyze_segment, value_to_biguint};
use crate::urlparse::{IndexSuffix, parse_url};

/// Encode a URL to the raw bitstream bytes.
pub fn encode_to_bits(url: &str) -> Result<Vec<u8>, Error> {
    let parsed = parse_url(url)?;
    let mut bs = WriteBitStream::new();

    // --- Header (12 bits) ---
    bs.write_bits(1, 4)?; // version 1
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

    // --- Host (2-bit mode) ---
    if let Some(idx) = lookup_host(&parsed.host) {
        // Mode 0: dictionary index.
        bs.write_bits(0, 2)?;
        bs.write_bits(idx as u64, 8)?;
    } else if parsed.host.bytes().all(|b| b.is_ascii_lowercase()) {
        // Mode 1: literal lowercase segment (base26-lower).
        bs.write_bits(1, 2)?;
        write_segment(&mut bs, &analyze_segment(&parsed.host), false)?;
    } else {
        // Mode 2: literal mixed-case segment (base62 or raw, self-describing).
        bs.write_bits(2, 2)?;
        write_segment(&mut bs, &analyze_segment(&parsed.host), false)?;
    }

    // --- TLD (1-bit mode) ---
    if parsed.tld.is_empty() {
        bs.write_bits(0, 1)?;
        bs.write_bits(TLD_ESCAPE as u64, 5)?;
    } else if let Some(idx) = lookup_tld(&parsed.tld) {
        bs.write_bits(0, 1)?;
        bs.write_bits(idx as u64, 5)?;
    } else {
        bs.write_bits(1, 1)?;
        write_segment(&mut bs, &analyze_segment(&parsed.tld), false)?;
    }

    // --- Index-suffix literal (only for Other(_)) ---
    if let IndexSuffix::Other(literal) = &parsed.index_suffix {
        write_segment(&mut bs, &analyze_segment(literal), true)?;
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
        bs.write_varint(segments.len() as u64)?;
        for s in segments {
            if let Some(token_idx) = lookup_path_token(s) {
                bs.write_bits(1, 1)?; // is_dict_token = 1
                bs.write_bits(token_idx as u64, 6)?;
            } else {
                bs.write_bits(0, 1)?; // is_dict_token = 0
                write_segment(&mut bs, &analyze_segment(s), true)?;
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
        bs.write_varint(parsed.query_segments.len() as u64)?;
        for (key, value) in &parsed.query_segments {
            // Key encoding
            if let Some(key_idx) = lookup_query_key(key) {
                bs.write_bits(1, 1)?; // is_dict_key = 1
                bs.write_bits(key_idx as u64, 6)?;
            } else {
                bs.write_bits(0, 1)?; // is_dict_key = 0
                write_segment(&mut bs, &analyze_segment(key), true)?;
            }

            // Value encoding
            match value {
                Some(v) => {
                    bs.write_bits(1, 1)?; // has_value = 1
                    if let Some(val_idx) = lookup_query_value(v) {
                        bs.write_bits(1, 1)?; // is_dict_val = 1
                        bs.write_bits(val_idx as u64, 5)?;
                    } else {
                        bs.write_bits(0, 1)?; // is_dict_val = 0
                        write_segment(&mut bs, &analyze_segment(v), true)?;
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
        bs.write_varint(parsed.fragment_segments.len() as u64)?;
        for s in &parsed.fragment_segments {
            write_segment(&mut bs, &analyze_segment(s), true)?;
        }
    }

    Ok(bs.into_bytes())
}

/// Encode a URL to its base85 wire representation.
///
/// # Examples
///
/// ```
/// let payload = urlz::encode("https://github.com/rust-lang/rust")?;
/// assert!(!payload.is_empty());
/// assert!(payload
/// .chars()
/// .all(|c| urlz::alphabet::BASE85_ALPHABET.contains(&(c as u8))));
/// # Ok::<(), urlz::Error>(())
/// ```
pub fn encode(url: &str) -> Result<String, Error> {
    let bits = encode_to_bits(url)?;
    let n = biguint_from_bytes_be(&bits);
    let payload = to_base(&n, BASE85_ALPHABET);
    // The decoder rejects payloads over 65536 bytes ; never emit a
    // payload the decoder would reject.
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

/// Wire size in bits of a varint-encoded `x`: groups of 7 data
/// bits, least-significant group first, each group written as 8 bits.
fn varint_bits(x: u64) -> u64 {
    let groups = if x == 0 {
        1
    } else {
        (64 - x.leading_zeros() as u64).div_ceil(7)
    };
    8 * groups
}

/// Write one segment: alphabet_id (4 bits), symbol_count varint,
/// value_bit_length varint, then the value's bits MSB-first. When
/// `huffman_allowed` and the huffman encoding is strictly smaller on the
/// wire than the base-N encoding, an alphabet_id-5 segment is written
/// instead: symbol_count varint, huffman bit length varint, then the
/// huffman codes.
fn write_segment(
    bs: &mut WriteBitStream,
    seg: &SegmentEncoding,
    huffman_allowed: bool,
) -> Result<(), Error> {
    // The decoder caps symbol_count at 4096; never emit a segment
    // the decoder would reject.
    if seg.symbol_count > MAX_SYMBOL_COUNT as usize {
        return Err(Error::InvalidUrl {
            reason: format!(
                "segment has {} symbols (max {MAX_SYMBOL_COUNT})",
                seg.symbol_count
            ),
        });
    }
    let value = value_to_biguint(seg)?;
    if huffman_allowed
        && seg.symbol_count > 0
        && let Ok(huff_len) = DEFAULT_HUFFMAN_ENCODER.bit_len(&seg.value)
    {
        let base_bit_len = value.bits();
        let huff_total = varint_bits(huff_len as u64) + huff_len as u64;
        let base_total = varint_bits(base_bit_len) + base_bit_len;
        if huff_total < base_total && (huff_len as u64) <= MAX_VALUE_BIT_LENGTH {
            bs.write_bits(5, 4)?;
            bs.write_varint(seg.symbol_count as u64)?;
            bs.write_varint(huff_len as u64)?;
            let written = DEFAULT_HUFFMAN_ENCODER.encode_into(bs, &seg.value)?;
            debug_assert_eq!(
                written, huff_len,
                "encode_into must write exactly huff_len bits"
            );
            return Ok(());
        }
    }
    bs.write_bits(seg.alphabet_id as u64, 4)?;
    bs.write_varint(seg.symbol_count as u64)?;
    let bit_len = value.bits() as u32;
    bs.write_varint(bit_len as u64)?;
    write_biguint_bits(bs, &value, bit_len)?;
    Ok(())
}

/// Write exactly `bit_len` bits of `value`, MSB-first, skipping the leading
/// zero bits of the minimal big-endian byte representation.
fn write_biguint_bits(bs: &mut WriteBitStream, value: &BigUint, bit_len: u32) -> Result<(), Error> {
    if bit_len == 0 {
        return Ok(());
    }
    let bytes = value.to_bytes_be();
    let total_bits = (bytes.len() as u32) * 8;
    let skip = total_bits - bit_len;
    let mut acc: u64 = 0;
    let mut acc_len: u32 = 0;
    let mut idx: u32 = 0;
    'bits: for byte in &bytes {
        for i in (0..8).rev() {
            if idx >= skip {
                let bit = ((byte >> i) & 1) as u64;
                acc = (acc << 1) | bit;
                acc_len += 1;
                if acc_len == 64 {
                    bs.write_bits(acc, 64)?;
                    acc = 0;
                    acc_len = 0;
                }
            }
            idx += 1;
            if idx == total_bits {
                break 'bits;
            }
        }
    }
    if acc_len > 0 {
        bs.write_bits(acc, acc_len)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::decode::decode;
    use crate::dict::{COMMON_HOSTS, KNOWN_TLDS};

    /// Assert that `decode(encode(u))` reproduces the normalized `u`.
    fn roundtrip(u: &str) {
        let expected = parse_url(u).unwrap().to_string();
        let encoded = encode(u).unwrap_or_else(|e| panic!("encode({u:?}) failed: {e}"));
        let decoded = decode(&encoded).unwrap_or_else(|e| panic!("decode({u:?}) failed: {e}"));
        assert_eq!(decoded, expected, "round-trip mismatch for {u:?}");
    }

    #[test]
    fn roundtrip_representative_urls() {
        // TLDs and hosts
        for tld in &KNOWN_TLDS[..31] {
            roundtrip(&format!("https://example.{tld}/"));
        }
        for host in &COMMON_HOSTS {
            roundtrip(&format!("https://{host}.com/"));
        }
        let urls = [
            "http://localhost",
            "https://127.0.0.1",
            "http://192.168.1.1/path",
            "https://example-site.com/x",
            "https://sub.domain.co.uk",
            "https://MySite.COM",
            "https://example.com:8080/path",
            "http://localhost:3000/api",
            "https://www.example.com/",
            "https://WWW.Example.com/",
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
            "https://example.com/日本語?q=かえで",
            "https://example.com/a,b(c)d!e*f;g",
            "https://example.com/path",
            "https://example.com/a!b",
        ];
        for url in urls {
            roundtrip(url);
        }

        // Long query roundtrip
        let q = "x".repeat(1000);
        roundtrip(&format!("https://example.com/?q={q}"));
    }

    #[test]
    fn default_port_is_dropped() {
        let encoded = encode("https://example.com:443/").unwrap();
        assert_eq!(decode(&encoded).unwrap(), "https://example.com/");
    }

    #[test]
    fn write_segment_huffman_decisions() {
        // "test" wins: huff 16 bits < base26 19 bits (varint-inclusive).
        let seg = SegmentEncoding {
            alphabet_id: 1,
            value: b"test".to_vec(),
            symbol_count: 4,
        };
        let mut bs = WriteBitStream::new();
        write_segment(&mut bs, &seg, true).unwrap();
        let bytes = bs.into_bytes();
        let mut r = crate::bitstream::ReadBitStream::from_bytes(&bytes);
        assert_eq!(r.read_bits(4).unwrap(), 5);
        assert_eq!(r.read_varint().unwrap(), 4);
        assert_eq!(r.read_varint().unwrap(), 16);

        // "x" (huff 6 > base 5) and "a" (huff 5 > base 0) stay base26.
        for s in ["x", "a"] {
            let seg = SegmentEncoding {
                alphabet_id: 1,
                value: s.as_bytes().to_vec(),
                symbol_count: s.len(),
            };
            let mut bs = WriteBitStream::new();
            write_segment(&mut bs, &seg, true).unwrap();
            let bytes = bs.into_bytes();
            let mut r = crate::bitstream::ReadBitStream::from_bytes(&bytes);
            assert_ne!(r.read_bits(4).unwrap(), 5, "segment {s:?} should stay base");
        }

        // Host literals are pinned to base26 by the format specification
        let seg = SegmentEncoding {
            alphabet_id: 1,
            value: b"path".to_vec(),
            symbol_count: 4,
        };
        let mut bs = WriteBitStream::new();
        write_segment(&mut bs, &seg, false).unwrap();
        let bytes = bs.into_bytes();
        let mut r = crate::bitstream::ReadBitStream::from_bytes(&bytes);
        assert_ne!(r.read_bits(4).unwrap(), 5);

        // Raw segment "a&b" wins: huff 17 bits < raw 23 bits ('&' = 6).
        let seg = SegmentEncoding {
            alphabet_id: 6,
            value: b"a&b".to_vec(),
            symbol_count: 3,
        };
        let mut bs = WriteBitStream::new();
        write_segment(&mut bs, &seg, true).unwrap();
        let bytes = bs.into_bytes();
        let mut r = crate::bitstream::ReadBitStream::from_bytes(&bytes);
        assert_eq!(r.read_bits(4).unwrap(), 5);
    }

    #[test]
    fn segment_and_payload_limits() {
        // Path segment count limits (max 64)
        let path_64 = (0..64)
            .map(|i| format!("s{i}"))
            .collect::<Vec<_>>()
            .join("/");
        roundtrip(&format!("https://example.com/{path_64}"));

        let path_65 = (0..65)
            .map(|i| format!("s{i}"))
            .collect::<Vec<_>>()
            .join("/");
        assert!(matches!(
            encode(&format!("https://example.com/{path_65}")),
            Err(Error::InvalidUrl { .. })
        ));

        // Symbol count limits (max 4096)
        roundtrip(&format!("https://example.com/{}", "x".repeat(4096)));
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
