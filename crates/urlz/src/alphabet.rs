//! Compression alphabets (base85) and arbitrary-precision base conversion.
//!
//! This module defines the base85 wire alphabet used by urlz payloads
//! ([`BASE85_ALPHABET`]), the 8-entry alphabet registry ([`ALPHABETS`]),
//! and the base-conversion primitives that turn [`BigUint`] values into
//! symbol strings and back.
//!
//! All alphabets are ASCII. Non-ASCII input is rejected by [`from_base`].

use crate::error::Error;
use num_bigint::BigUint;
use num_traits::Zero;

/// The 85-character base85 alphabet.
///
/// All printable ASCII 0x21–0x7E EXCLUDING `" ' \ % + / = < >`.
pub const BASE85_ALPHABET: &[u8; 85] =
    b"!#$&()*,-.0123456789:;?@ABCDEFGHIJKLMNOPQRSTUVWXYZ[]^_`abcdefghijklmnopqrstuvwxyz{|}~";

/// One entry in the alphabet registry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AlphabetInfo {
    /// 4-bit alphabet id (0..=7).
    pub id: u8,
    pub name: &'static str,
    /// The alphabet's symbol bytes. Empty for huffman-mode, raw-fallback,
    /// and reserved (they do not use base-N conversion).
    pub chars: &'static [u8],
}

/// The 8-entry alphabet registry.
///
/// Entries 0–4 are base-N alphabets usable with [`to_base`]/[`from_base`].
/// Entry 5 (huffman-mode) operates on [`BASE85_ALPHABET`] symbols but is
/// Huffman-compressed rather than base-converted, so it carries no char slice
/// here. Entry 6 (raw-fallback) covers all 256 byte values and entry 7
/// (reserved) is unused.
pub const ALPHABETS: [AlphabetInfo; 8] = [
    AlphabetInfo {
        id: 0,
        name: "base10",
        chars: b"0123456789",
    },
    AlphabetInfo {
        id: 1,
        name: "base26-lower",
        chars: b"abcdefghijklmnopqrstuvwxyz",
    },
    AlphabetInfo {
        id: 2,
        name: "base36",
        chars: b"0123456789abcdefghijklmnopqrstuvwxyz",
    },
    AlphabetInfo {
        id: 3,
        name: "base62",
        chars: b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz",
    },
    AlphabetInfo {
        id: 4,
        name: "base64url",
        chars: b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_",
    },
    AlphabetInfo {
        id: 5,
        name: "huffman-mode",
        chars: &[],
    },
    AlphabetInfo {
        id: 6,
        name: "raw-fallback",
        chars: &[],
    },
    AlphabetInfo {
        id: 7,
        name: "reserved",
        chars: &[],
    },
];

const fn make_inv_table(alphabet: &[u8]) -> [i8; 256] {
    let mut table = [-1i8; 256];
    let mut i = 0;
    while i < alphabet.len() {
        table[alphabet[i] as usize] = i as i8;
        i += 1;
    }
    table
}

pub const BASE85_INV: [i8; 256] = make_inv_table(BASE85_ALPHABET);
const ALPHABET_INV: [[i8; 256]; 5] = [
    make_inv_table(ALPHABETS[0].chars),
    make_inv_table(ALPHABETS[1].chars),
    make_inv_table(ALPHABETS[2].chars),
    make_inv_table(ALPHABETS[3].chars),
    make_inv_table(ALPHABETS[4].chars),
];

#[inline(always)]
pub fn char_index_base85(c: u8) -> Option<usize> {
    let idx = BASE85_INV[c as usize];
    if idx >= 0 { Some(idx as usize) } else { None }
}

#[inline]
pub fn char_index(c: u8, alphabet: &[u8]) -> Option<usize> {
    let idx = match alphabet.len() {
        10 if alphabet == ALPHABETS[0].chars => ALPHABET_INV[0][c as usize],
        26 if alphabet == ALPHABETS[1].chars => ALPHABET_INV[1][c as usize],
        36 if alphabet == ALPHABETS[2].chars => ALPHABET_INV[2][c as usize],
        62 if alphabet == ALPHABETS[3].chars => ALPHABET_INV[3][c as usize],
        64 if alphabet == ALPHABETS[4].chars => ALPHABET_INV[4][c as usize],
        85 if alphabet == BASE85_ALPHABET => BASE85_INV[c as usize],
        _ => return alphabet.iter().position(|&a| a == c),
    };
    if idx >= 0 { Some(idx as usize) } else { None }
}

/// Convert a big integer to a symbol string in `alphabet` (big-endian digits).
///
/// `0` maps to the single first symbol of the alphabet (never empty).
/// The alphabet must be non-empty.
pub fn to_base(v: &BigUint, alphabet: &[u8]) -> String {
    debug_assert!(!alphabet.is_empty(), "to_base: alphabet must not be empty");
    let base = alphabet.len();
    if v.is_zero() {
        return String::from(alphabet[0] as char);
    }
    let mut digits = Vec::new();
    let mut n = v.clone();
    while !n.is_zero() {
        let q = &n / base;
        let r = &n % base;
        // r < base ≤ 256, so it always fits in a single u32 digit.
        let idx = r.iter_u32_digits().next().unwrap_or(0) as usize;
        digits.push(alphabet[idx]);
        n = q;
    }
    digits.reverse();
    String::from_utf8(digits).expect("alphabet symbols are valid ASCII")
}

/// Parse a symbol string in `alphabet` into a big integer.
///
/// Rejects empty strings (`InvalidPayload`) and any character not present in
/// `alphabet` (`UnsupportedCharacter`).
pub fn from_base(s: &str, alphabet: &[u8]) -> Result<BigUint, Error> {
    if s.is_empty() {
        return Err(Error::InvalidPayload {
            reason: "empty string".to_string(),
        });
    }
    debug_assert!(
        !alphabet.is_empty(),
        "from_base: alphabet must not be empty"
    );
    let base = BigUint::from(alphabet.len());
    let mut result = BigUint::zero();
    for c in s.chars() {
        let idx = if c.is_ascii() {
            char_index(c as u8, alphabet)
        } else {
            None
        };
        let idx = idx.ok_or(Error::UnsupportedCharacter(c))?;
        result = result * &base + BigUint::from(idx);
    }
    Ok(result)
}

pub fn biguint_from_bytes_be(bytes: &[u8]) -> BigUint {
    BigUint::from_bytes_be(bytes)
}

/// Minimal big-endian byte representation of `v`.
///
/// `0` produces an **empty** vector (leading zero bytes are dropped). Note that
/// `num-bigint`'s `to_bytes_be()` returns `[0]` for zero, so this normalizes it
/// to empty. Callers that need to preserve leading zero bytes must reconstruct
/// the exact length from the bitstream field structure instead.
pub fn bytes_from_biguint_be(v: &BigUint) -> Vec<u8> {
    if v.is_zero() {
        Vec::new()
    } else {
        v.to_bytes_be()
    }
}

/// Convert a segment's symbol string to a big integer, returning the symbol
/// count alongside.
///
/// `symbol_count` is `s.len()` (all alphabets are ASCII, so this equals the
/// number of symbols). **Leading symbols equal to `alphabet[0]` are
/// significant** — they are lost in the integer conversion, so the caller must
/// use `symbol_count` to reconstruct the exact original string.
pub fn segment_to_biguint(s: &str, alphabet: &[u8]) -> Result<(BigUint, usize), Error> {
    let value = from_base(s, alphabet)?;
    Ok((value, s.len()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn base85_alphabet_exact() {
        assert_eq!(BASE85_ALPHABET.len(), 85);
        assert_eq!(
 BASE85_ALPHABET,
 b"!#$&()*,-.0123456789:;?@ABCDEFGHIJKLMNOPQRSTUVWXYZ[]^_`abcdefghijklmnopqrstuvwxyz{|}~"
 );
        // All printable ASCII 0x21..=0x7E except the 9 excluded chars, no dupes.
        let excluded = *b"\"'\\%+/=<>";
        let mut seen = [false; 256];
        for &b in BASE85_ALPHABET.iter() {
            assert!(
                (0x21..=0x7E).contains(&b),
                "byte 0x{:02X} out of printable range",
                b
            );
            assert!(
                !excluded.contains(&b),
                "byte 0x{:02X} should be excluded",
                b
            );
            assert!(!seen[b as usize], "duplicate byte 0x{:02X}", b);
            seen[b as usize] = true;
        }
        for b in 0x21..=0x7E {
            if !excluded.contains(&b) {
                assert!(seen[b as usize], "missing byte 0x{:02X}", b);
            }
        }
    }

    #[test]
    fn alphabet_registry() {
        assert_eq!(ALPHABETS.len(), 8);
        for (i, info) in ALPHABETS.iter().enumerate() {
            assert_eq!(info.id as usize, i);
        }
        assert_eq!(ALPHABETS[0].chars, b"0123456789");
        assert_eq!(ALPHABETS[1].chars, b"abcdefghijklmnopqrstuvwxyz");
        assert_eq!(ALPHABETS[2].chars, b"0123456789abcdefghijklmnopqrstuvwxyz");
        assert_eq!(
            ALPHABETS[3].chars,
            b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz"
        );
        assert_eq!(
            ALPHABETS[4].chars,
            b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_"
        );
        assert!(ALPHABETS[5].chars.is_empty());
        assert!(ALPHABETS[6].chars.is_empty());
        assert!(ALPHABETS[7].chars.is_empty());
    }

    #[test]
    fn char_index_works() {
        assert_eq!(char_index(b'!', BASE85_ALPHABET), Some(0));
        assert_eq!(char_index(b'0', BASE85_ALPHABET), Some(10));
        assert_eq!(char_index(b'~', BASE85_ALPHABET), Some(84));
        assert_eq!(char_index(b' ', BASE85_ALPHABET), None);
    }

    #[test]
    fn known_vectors() {
        assert_eq!(to_base(&BigUint::zero(), BASE85_ALPHABET), "!");
        assert_eq!(to_base(&BigUint::from(84u64), BASE85_ALPHABET), "~");
        assert_eq!(to_base(&BigUint::from(85u64), BASE85_ALPHABET), "#!");
        assert_eq!(
            from_base("~", BASE85_ALPHABET).unwrap(),
            BigUint::from(84u64)
        );
        assert_eq!(
            from_base("#!", BASE85_ALPHABET).unwrap(),
            BigUint::from(85u64)
        );
    }

    #[test]
    fn to_base_zero_is_single_first_symbol() {
        for info in &ALPHABETS[..5] {
            let s = to_base(&BigUint::zero(), info.chars);
            assert_eq!(s, String::from(info.chars[0] as char));
            assert_eq!(s.len(), 1);
        }
    }

    #[test]
    fn known_roundtrips() {
        let cases = [
            BigUint::zero(),
            BigUint::from(1u64),
            BigUint::from(9u64),
            BigUint::from(10u64),
            BigUint::from(84u64),
            BigUint::from(85u64),
            BigUint::from(123456789u64),
            BigUint::from(u64::MAX),
            BigUint::from_bytes_be(&[0xFF; 32]),
            BigUint::from_bytes_be(&[0x00, 0x12, 0x34, 0x56]),
        ];
        for v in &cases {
            for info in &ALPHABETS[..5] {
                let s = to_base(v, info.chars);
                assert!(!s.is_empty());
                let back = from_base(&s, info.chars).unwrap();
                assert_eq!(back, *v, "roundtrip failed for alphabet {}", info.name);
            }
        }
    }

    #[test]
    fn from_base_rejects_invalid() {
        assert!(from_base("", BASE85_ALPHABET).is_err());
        assert!(matches!(
            from_base("12a", b"0123456789"),
            Err(Error::UnsupportedCharacter('a'))
        ));
        assert!(matches!(
            from_base("abc%", BASE85_ALPHABET),
            Err(Error::UnsupportedCharacter('%'))
        ));
        assert!(matches!(
            from_base("café", BASE85_ALPHABET),
            Err(Error::UnsupportedCharacter('é'))
        ));
    }

    #[test]
    fn segment_to_biguint_counts_symbols() {
        let (v, count) = segment_to_biguint("abc", b"abcdefghijklmnopqrstuvwxyz").unwrap();
        assert_eq!(count, 3);
        assert_eq!(v, BigUint::from(1u64) * 26u64 + 2u64);
        // Leading alphabet[0] symbols are significant for the count.
        let (v2, count2) = segment_to_biguint("00123", b"0123456789").unwrap();
        assert_eq!(count2, 5);
        assert_eq!(v2, BigUint::from(123u64)); // leading zeros lost in value
        assert!(segment_to_biguint("", b"0123456789").is_err());
    }

    proptest! {
    #[test]
    fn roundtrip_biguint_to_base(v_bytes in any::<Vec<u8>>(), alpha_idx in 0..5usize) {
    let alphabet = ALPHABETS[alpha_idx].chars;
    let v = BigUint::from_bytes_be(&v_bytes);
    let s = to_base(&v, alphabet);
    let back = from_base(&s, alphabet).unwrap();
    prop_assert_eq!(back, v);
    }

    #[test]
    fn roundtrip_string_from_base(
    alpha_idx in 0..5usize,
    indices in proptest::collection::vec(any::<u8>(), 1..64),
    ) {
    let alphabet = ALPHABETS[alpha_idx].chars;
    let base = alphabet.len() as u8;
    let mut s = String::new();
    for (i, &idx) in indices.iter().enumerate() {
    // First symbol must not be alphabet[0]: leading zeros are lost
    // in the integer conversion and cannot round-trip.
    let idx = if i == 0 { (idx % (base - 1)) + 1 } else { idx % base };
    s.push(alphabet[idx as usize] as char);
    }
    let (v, count) = segment_to_biguint(&s, alphabet).unwrap();
    prop_assert_eq!(count, s.len());
    prop_assert_eq!(to_base(&v, alphabet), s);
    }
    }
}
