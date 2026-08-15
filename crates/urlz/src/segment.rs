//! Segment analyzer: selects the smallest alphabet for a URL segment.
//!
//! [`analyze_segment`] inspects a segment string and returns a
//! [`SegmentEncoding`] describing how the encoder should serialize it:
//! which alphabet, the raw symbol bytes, and the symbol count.
//!
//! The alphabet registry is defined in [`crate::alphabet::ALPHABETS`]
//! and is **not** duplicated here. Selection order (smallest → largest):
//! base10 < base26-lower < base36 < base62 < base64url. If no base alphabet
//! contains every byte of the segment, the segment falls back to raw bytes
//! (alphabet_id 6).

use num_bigint::BigUint;
use num_traits::Zero;

use crate::alphabet::{ALPHABETS, biguint_from_bytes_be, char_index, segment_to_biguint};
use crate::error::Error;

/// The encoding of a single URL segment.
///
/// - `alphabet_id`: 4-bit alphabet id (0–7).
/// - `value`: raw symbol bytes in that alphabet. For base alphabets (0–4) these are the segment's ASCII bytes; for raw-fallback (6) they are the literal UTF-8 bytes. The encoder converts them to a [`BigUint`] via [`value_to_biguint`].
/// - `symbol_count`: number of symbols (bytes) in the segment. Leading `alphabet[0]` symbols are lost in the integer conversion, so the decoder uses this count to reconstruct the exact string.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SegmentEncoding {
    pub alphabet_id: u8,
    pub value: Vec<u8>,
    pub symbol_count: usize,
}

/// Analyzes a segment string and selects the smallest alphabet that contains
/// every byte.
///
/// Selection order: base10 < base26-lower < base36 < base62 < base64url.
/// If none fit, the segment is stored as raw bytes (alphabet_id 6).
///
/// The empty segment is encoded as `alphabet_id = 0` (base10), an empty
/// `value`, and `symbol_count = 0`; it round-trips to the empty string.
pub fn analyze_segment(s: &str) -> SegmentEncoding {
    if s.is_empty() {
        return SegmentEncoding {
            alphabet_id: 0,
            value: Vec::new(),
            symbol_count: 0,
        };
    }
    for info in &ALPHABETS[..5] {
        if s.bytes().all(|b| char_index(b, info.chars).is_some()) {
            return SegmentEncoding {
                alphabet_id: info.id,
                value: s.as_bytes().to_vec(),
                symbol_count: s.len(),
            };
        }
    }
    SegmentEncoding {
        alphabet_id: 6,
        value: s.as_bytes().to_vec(),
        symbol_count: s.len(),
    }
}

/// Converts a segment's raw symbol bytes to its integer value (the
/// `symbols_to_biguint` step).
///
/// For base alphabets (0–4) the bytes are interpreted as a big-endian number
/// in the alphabet's base; leading `alphabet[0]` symbols are lost in the
/// integer and must be restored via `symbol_count`. For raw-fallback (6) the
/// bytes are interpreted as a big-endian integer directly (base 256).
///
/// Value → bits: the encoder writes `value_bit_length =
/// value.bits()` as a varint, then the value's bits MSB-first. The decoder
/// reads those bits back into a [`BigUint`] and reconstructs the symbols using
/// `symbol_count`, padding with leading `alphabet[0]` symbols.
pub fn value_to_biguint(enc: &SegmentEncoding) -> Result<BigUint, Error> {
    if enc.value.is_empty() {
        return Ok(BigUint::zero());
    }
    match enc.alphabet_id {
        0..=4 => {
            let alphabet = ALPHABETS[enc.alphabet_id as usize].chars;
            let s = std::str::from_utf8(&enc.value).map_err(|_| Error::InvalidPayload {
                reason: "segment value is not ASCII".to_string(),
            })?;
            segment_to_biguint(s, alphabet).map(|(v, _)| v)
        }
        6 => Ok(biguint_from_bytes_be(&enc.value)),
        _ => Err(Error::InvalidPayload {
            reason: format!("alphabet_id {} has no integer value", enc.alphabet_id),
        }),
    }
}

/// Reconstructs the original segment string from an encoding (the
/// `symbols_to_string` step).
///
/// For base alphabets (0–4) each value byte is mapped through the alphabet.
/// For raw-fallback (6) the value bytes are the literal UTF-8 bytes and are
/// returned as-is.
pub fn segment_to_string(enc: &SegmentEncoding) -> Result<String, Error> {
    match enc.alphabet_id {
        0..=4 => {
            let alphabet = ALPHABETS[enc.alphabet_id as usize].chars;
            for &b in &enc.value {
                if char_index(b, alphabet).is_none() {
                    return Err(Error::UnsupportedCharacter(b as char));
                }
            }
            String::from_utf8(enc.value.clone()).map_err(|_| Error::InvalidPayload {
                reason: "segment value is not ASCII".to_string(),
            })
        }
        6 => String::from_utf8(enc.value.clone()).map_err(|_| Error::InvalidPayload {
            reason: "raw segment is not valid UTF-8".to_string(),
        }),
        _ => Err(Error::InvalidPayload {
            reason: format!(
                "alphabet_id {} cannot be decoded to a string",
                enc.alphabet_id
            ),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::alphabet::{bytes_from_biguint_be, to_base};
    use proptest::prelude::*;

    /// Asserts the full encoding of `s` and returns it for further checks.
    fn assert_encoding(s: &str, alphabet_id: u8, symbol_count: usize) -> SegmentEncoding {
        let enc = analyze_segment(s);
        assert_eq!(enc.alphabet_id, alphabet_id, "alphabet_id for {s:?}");
        assert_eq!(enc.symbol_count, symbol_count, "symbol_count for {s:?}");
        assert_eq!(enc.value, s.as_bytes(), "value bytes for {s:?}");
        enc
    }

    /// Reconstructs the segment string through the integer path: value →
    /// BigUint → symbols, padded to `symbol_count` with leading `alphabet[0]`
    /// symbols.
    fn reconstruct_via_biguint(enc: &SegmentEncoding) -> String {
        if enc.symbol_count == 0 {
            return String::new();
        }
        let v = value_to_biguint(enc).unwrap();
        match enc.alphabet_id {
            0..=4 => {
                let alphabet = ALPHABETS[enc.alphabet_id as usize].chars;
                let mut digits = to_base(&v, alphabet).into_bytes();
                while digits.len() < enc.symbol_count {
                    digits.insert(0, alphabet[0]);
                }
                digits.iter().map(|&b| b as char).collect()
            }
            6 => {
                let mut bytes = bytes_from_biguint_be(&v);
                while bytes.len() < enc.symbol_count {
                    bytes.insert(0, 0);
                }
                String::from_utf8(bytes).unwrap()
            }
            _ => unreachable!(
                "analyze_segment never produces alphabet_id {}",
                enc.alphabet_id
            ),
        }
    }

    #[test]
    fn selects_expected_alphabet_for_inputs() {
        let cases = [
            ("123", 0, 3),          // base10
            ("abc", 1, 3),          // base26-lower
            ("a9", 2, 2),           // base36
            ("Ab3", 3, 3),          // base62
            ("abc-", 4, 4),         // base64url
            ("a.b", 6, 3),          // raw fallback (dot)
            ("hello world", 6, 11), // raw fallback (space)
            ("日本語", 6, 9),       // raw fallback (unicode UTF-8 bytes)
        ];
        for (input, alphabet_id, symbol_count) in cases {
            assert_encoding(input, alphabet_id, symbol_count);
        }
    }

    #[test]
    fn empty_segment() {
        let enc = analyze_segment("");
        assert_eq!(enc.alphabet_id, 0);
        assert_eq!(enc.symbol_count, 0);
        assert!(enc.value.is_empty());
        assert_eq!(segment_to_string(&enc).unwrap(), "");
        assert_eq!(value_to_biguint(&enc).unwrap(), BigUint::zero());
    }

    #[test]
    fn value_to_biguint_semantics() {
        // "123" in base10 → 123.
        assert_eq!(
            value_to_biguint(&analyze_segment("123")).unwrap(),
            BigUint::from(123u64)
        );
        // "abc" in base26 → a=0, b=1, c=2 → 0*26² + 1*26 + 2 = 28.
        assert_eq!(
            value_to_biguint(&analyze_segment("abc")).unwrap(),
            BigUint::from(28u64)
        );
        // Leading alphabet[0] symbols are lost in the integer but preserved
        // by symbol_count.
        let enc = analyze_segment("00123");
        assert_eq!(value_to_biguint(&enc).unwrap(), BigUint::from(123u64));
        assert_eq!(enc.symbol_count, 5);
        // Raw fallback: bytes interpreted as a big-endian integer.
        let enc = analyze_segment("a.b");
        assert_eq!(
            value_to_biguint(&enc).unwrap(),
            BigUint::from_bytes_be(b"a.b")
        );
    }

    #[test]
    fn roundtrip_via_symbol_bytes() {
        for s in [
            "123",
            "abc",
            "a9",
            "Ab3",
            "abc-",
            "a.b",
            "hello world",
            "日本語",
            "",
        ] {
            let enc = analyze_segment(s);
            assert_eq!(segment_to_string(&enc).unwrap(), s, "roundtrip for {s:?}");
        }
    }

    #[test]
    fn roundtrip_via_biguint() {
        for s in [
            "123",
            "abc",
            "a9",
            "Ab3",
            "abc-",
            "00123",
            "000abc",
            "a.b",
            "hello world",
            "",
        ] {
            let enc = analyze_segment(s);
            assert_eq!(reconstruct_via_biguint(&enc), s, "roundtrip for {s:?}");
        }
    }

    proptest! {
    #[test]
    fn analyze_roundtrips_symbol_bytes(s in ".*") {
    let enc = analyze_segment(&s);
    prop_assert_eq!(segment_to_string(&enc).unwrap(), s);
    }

    #[test]
    fn analyze_roundtrips_biguint(s in "[0-9a-zA-Z_-]*") {
    let enc = analyze_segment(&s);
    prop_assert_eq!(reconstruct_via_biguint(&enc), s);
    }
    }
}
