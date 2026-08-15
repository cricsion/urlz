//! urlz URL decoder.
//!
//! [`decode`] accepts a base85 payload and converts it to raw bitstream bytes.
//! [`decode_bits`] parses the wire format bit layout and reconstructs the
//! normalized URL.
//!
//! Decoding is hardened against malformed payloads: version and dict_set_id
//! are validated, resource limits are enforced, and trailing padding must be
//! all zero.

use num_bigint::BigUint;
use num_traits::Zero;

use crate::alphabet::{ALPHABETS, BASE85_ALPHABET, bytes_from_biguint_be, from_base, to_base};
use crate::bitstream::ReadBitStream;
use crate::dict::{
    COMMON_HOSTS, COMMON_PATH_TOKENS, COMMON_QUERY_KEYS, COMMON_QUERY_VALUES, DICT_SET_ID,
    HOST_ESCAPE, TLD_ESCAPE, host_at, path_token_at, query_key_at, query_value_at, tld_at,
};
use crate::error::Error;
use crate::huffman::DEFAULT_HUFFMAN_DECODER;
use crate::segment::{SegmentEncoding, segment_to_string};

/// Maximum accepted payload length in bytes .
pub(crate) const MAX_PAYLOAD_BYTES: usize = 65536;
/// Maximum segment count per region.
pub(crate) const MAX_SEGMENT_COUNT: u64 = 64;
/// Maximum symbol count per segment.
pub(crate) const MAX_SYMBOL_COUNT: u64 = 4096;
/// Absolute cap on a segment's value bit length.
pub(crate) const MAX_VALUE_BIT_LENGTH: u64 = 65536;
/// Maximum decoded host/tld length.
const MAX_HOST_TLD_LEN: usize = 4096;

/// Decode a base85 payload back into the normalized URL.
///
/// # Examples
///
/// ```
/// let payload = urlz::encode("https://github.com/rust-lang/rust")?;
/// assert_eq!(urlz::decode(&payload)?, "https://github.com/rust-lang/rust");
/// # Ok::<(), urlz::Error>(())
/// ```
pub fn decode(payload: &str) -> Result<String, Error> {
    if payload.len() > MAX_PAYLOAD_BYTES {
        return Err(Error::InvalidPayload {
            reason: "payload too large".to_string(),
        });
    }
    let n = from_base(payload, BASE85_ALPHABET)?;
    let bits = bytes_from_biguint_be(&n);
    decode_bits(&bits)
}

/// Parse a raw bitstream and reconstruct the normalized URL.
pub fn decode_bits(bits: &[u8]) -> Result<String, Error> {
    let mut bs = ReadBitStream::from_bytes(bits);

    let version = bs.read_bits(4)? as u8;
    if version != 1 {
        return Err(Error::UnsupportedVersion(version));
    }

    let dict_set_id = bs.read_bits(4)? as u8;
    if dict_set_id != DICT_SET_ID {
        return Err(Error::InvalidPayload {
            reason: "unknown dict set".to_string(),
        });
    }
    let https = bs.read_bits(1)? == 1;
    let www = bs.read_bits(1)? == 1;
    let index_code = bs.read_bits(2)? as u8;

    let host_mode = bs.read_bits(2)? as u8;
    let host = match host_mode {
        0 => {
            let idx = bs.read_bits(8)? as u8;
            if idx == HOST_ESCAPE {
                read_segment(&mut bs)?
            } else if idx as usize >= COMMON_HOSTS.len() {
                return Err(Error::InvalidPayload {
                    reason: format!("host dictionary index {idx} out of range"),
                });
            } else {
                host_at(idx).to_string()
            }
        }
        1 | 2 => read_segment(&mut bs)?,
        3 => {
            return Err(Error::InvalidPayload {
                reason: "reserved host mode".to_string(),
            });
        }
        _ => {
            return Err(Error::InvalidPayload {
                reason: "invalid host mode".to_string(),
            });
        }
    };
    if host.len() > MAX_HOST_TLD_LEN {
        return Err(Error::InvalidPayload {
            reason: "host too long".to_string(),
        });
    }

    let tld_mode = bs.read_bits(1)? as u8;
    let tld = match tld_mode {
        0 => {
            let idx = bs.read_bits(5)? as u8;
            if idx == TLD_ESCAPE {
                String::new()
            } else {
                tld_at(idx).to_string()
            }
        }
        1 => read_segment(&mut bs)?,
        _ => {
            return Err(Error::InvalidPayload {
                reason: "invalid tld mode".to_string(),
            });
        }
    };
    if tld.len() > MAX_HOST_TLD_LEN {
        return Err(Error::InvalidPayload {
            reason: "tld too long".to_string(),
        });
    }

    let index_suffix: Option<String> = match index_code {
        0 => None,
        1 => Some("index.html".to_string()),
        2 => Some("index.php".to_string()),
        3 => Some(read_segment(&mut bs)?),
        _ => {
            return Err(Error::InvalidPayload {
                reason: "invalid index suffix code".to_string(),
            });
        }
    };

    let path_present = bs.read_bits(1)? == 1;
    let query_present = bs.read_bits(1)? == 1;
    let fragment_present = bs.read_bits(1)? == 1;

    let mut path_segments: Vec<String> = Vec::new();
    if path_present {
        let count = read_varint_checked(&mut bs, MAX_SEGMENT_COUNT, "segment count")?;
        for _ in 0..count {
            let is_dict_token = bs.read_bits(1)? == 1;
            if is_dict_token {
                let token_idx = bs.read_bits(6)? as u8;
                if token_idx as usize >= COMMON_PATH_TOKENS.len() {
                    return Err(Error::InvalidPayload {
                        reason: format!("path token index {token_idx} out of range"),
                    });
                }
                path_segments.push(path_token_at(token_idx).to_string());
            } else {
                path_segments.push(read_segment(&mut bs)?);
            }
        }
    }

    let mut query_segments: Vec<(String, Option<String>)> = Vec::new();
    if query_present {
        let count = read_varint_checked(&mut bs, MAX_SEGMENT_COUNT, "segment count")?;
        for _ in 0..count {
            // Key
            let is_dict_key = bs.read_bits(1)? == 1;
            let key = if is_dict_key {
                let key_idx = bs.read_bits(6)? as u8;
                if key_idx as usize >= COMMON_QUERY_KEYS.len() {
                    return Err(Error::InvalidPayload {
                        reason: format!("query key index {key_idx} out of range"),
                    });
                }
                query_key_at(key_idx).to_string()
            } else {
                read_segment(&mut bs)?
            };

            // Value
            let has_val = bs.read_bits(1)? == 1;
            let value = if has_val {
                let is_dict_val = bs.read_bits(1)? == 1;
                let val_str = if is_dict_val {
                    let val_idx = bs.read_bits(5)? as u8;
                    if val_idx as usize >= COMMON_QUERY_VALUES.len() {
                        return Err(Error::InvalidPayload {
                            reason: format!("query value index {val_idx} out of range"),
                        });
                    }
                    query_value_at(val_idx).to_string()
                } else {
                    read_segment(&mut bs)?
                };
                Some(val_str)
            } else {
                None
            };

            query_segments.push((key, value));
        }
    }

    let mut fragment_segments: Vec<String> = Vec::new();
    if fragment_present {
        let count = read_varint_checked(&mut bs, MAX_SEGMENT_COUNT, "segment count")?;
        for _ in 0..count {
            fragment_segments.push(read_segment(&mut bs)?);
        }
    }

    if !bs.read_remaining_all_zero() {
        return Err(Error::InvalidPayload {
            reason: "non-zero padding".to_string(),
        });
    }

    let mut result = String::new();
    result.push_str(if https { "https" } else { "http" });
    result.push_str("://");
    if www {
        result.push_str("www.");
    }
    result.push_str(&host);
    if !tld.is_empty() {
        result.push('.');
        result.push_str(&tld);
    }
    if !path_segments.is_empty() {
        result.push('/');
        result.push_str(&path_segments.join("/"));
    }
    if let Some(suffix) = &index_suffix {
        result.push('/');
        result.push_str(suffix);
    }
    if query_present {
        result.push('?');
        let pairs: Vec<String> = query_segments
            .iter()
            .map(|(key, value)| match value {
                Some(v) => format!("{}={}", key, v),
                None => key.clone(),
            })
            .collect();
        result.push_str(&pairs.join("&"));
    }
    if fragment_present {
        result.push('#');
        result.push_str(&fragment_segments.join("/"));
    }
    Ok(result)
}

/// Read one segment: alphabet_id (4 bits), symbol_count varint,
/// value_bit_length varint, then the value's bits MSB-first. Returns the
/// reconstructed segment string.
fn read_segment(bs: &mut ReadBitStream) -> Result<String, Error> {
    let alphabet_id = bs.read_bits(4)? as u8;
    let symbol_count = read_varint_checked(bs, MAX_SYMBOL_COUNT, "symbol count")? as usize;
    let value_bit_length = read_varint_checked(bs, MAX_VALUE_BIT_LENGTH, "value bit length")?;
    if alphabet_id != 5 && value_bit_length > (symbol_count as u64) * 8 {
        return Err(Error::InvalidPayload {
            reason: "value bit length exceeds symbol capacity".to_string(),
        });
    }
    let value = read_biguint_bits(bs, value_bit_length)?;

    if alphabet_id == 5 {
        // Forward compatibility: huffman-compressed segment.
        // `value` holds exactly `value_bit_length` bits read MSB-first, but
        // `to_bytes_be` drops leading zero bytes and left-aligns the rest — a
        // huffman stream can legitimately start with zero bits (e.g. code
        // `000`) and need not be byte-aligned, so shift the value left to
        // right-align the stream before converting back to bytes.
        let byte_len = (value_bit_length as usize).div_ceil(8);
        let shift = (byte_len as u64) * 8 - value_bit_length;
        let mut bytes = (value << shift).to_bytes_be();
        if bytes.len() < byte_len {
            let missing = byte_len - bytes.len();
            let mut padded = Vec::with_capacity(byte_len);
            padded.extend(core::iter::repeat_n(0u8, missing));
            padded.extend_from_slice(&bytes);
            bytes = padded;
        }
        let decoded = DEFAULT_HUFFMAN_DECODER.decode(&bytes, symbol_count)?;
        return String::from_utf8(decoded).map_err(|_| Error::InvalidPayload {
            reason: "huffman segment is not valid UTF-8".to_string(),
        });
    }

    let symbols = biguint_to_symbols(alphabet_id, &value, symbol_count)?;
    segment_to_string(&SegmentEncoding {
        alphabet_id,
        value: symbols,
        symbol_count,
    })
}

#[inline]
fn read_varint_checked(bs: &mut ReadBitStream, max: u64, what: &str) -> Result<u64, Error> {
    let n = bs.read_varint()?;
    if n > max {
        return Err(Error::InvalidPayload {
            reason: format!("{} too large", what),
        });
    }
    Ok(n)
}

/// Read `bit_len` bits MSB-first into a [`BigUint`], chunked at 64 bits.
#[inline]
fn read_biguint_bits(bs: &mut ReadBitStream, bit_len: u64) -> Result<BigUint, Error> {
    let mut result = BigUint::zero();
    let mut remaining = bit_len;
    while remaining > 0 {
        let chunk = remaining.min(64) as u32;
        let bits = bs.read_bits(chunk)?;
        result = (result << chunk) | BigUint::from(bits);
        remaining -= chunk as u64;
    }
    Ok(result)
}

/// Reconstruct a segment's symbol bytes from its integer value, padding with
/// leading `alphabet[0]` symbols (or zero bytes for raw) to `symbol_count`.
fn biguint_to_symbols(
    alphabet_id: u8,
    value: &BigUint,
    symbol_count: usize,
) -> Result<Vec<u8>, Error> {
    if symbol_count == 0 {
        return Ok(Vec::new());
    }
    match alphabet_id {
        0..=4 => {
            let alphabet = ALPHABETS[alphabet_id as usize].chars;
            let digits = to_base(value, alphabet).into_bytes();
            if digits.len() < symbol_count {
                let missing = symbol_count - digits.len();
                let mut padded = Vec::with_capacity(symbol_count);
                padded.extend(core::iter::repeat_n(alphabet[0], missing));
                padded.extend_from_slice(&digits);
                Ok(padded)
            } else {
                Ok(digits)
            }
        }
        6 => {
            let bytes = bytes_from_biguint_be(value);
            if bytes.len() < symbol_count {
                let missing = symbol_count - bytes.len();
                let mut padded = Vec::with_capacity(symbol_count);
                padded.extend(core::iter::repeat_n(0u8, missing));
                padded.extend_from_slice(&bytes);
                Ok(padded)
            } else {
                Ok(bytes)
            }
        }
        _ => Err(Error::InvalidPayload {
            reason: format!("alphabet_id {} cannot be decoded to a string", alphabet_id),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::encode::encode;

    #[test]
    fn rejects_wrong_version() {
        // Craft a bitstream with version = 3: header is 12 bits, version is
        // the first 4. Write version 3, then valid-looking rest.
        let mut bs = crate::bitstream::WriteBitStream::new();
        bs.write_bits(3, 4).unwrap();
        bs.write_bits(DICT_SET_ID as u64, 4).unwrap();
        bs.write_bits(1, 1).unwrap();
        bs.write_bits(0, 1).unwrap();
        bs.write_bits(0, 2).unwrap();
        bs.write_bits(0, 2).unwrap(); // host mode 0
        bs.write_bits(0, 8).unwrap(); // host index
        bs.write_bits(0, 1).unwrap(); // tld mode 0
        bs.write_bits(TLD_ESCAPE as u64, 5).unwrap();
        bs.write_bits(0, 3).unwrap(); // no resource regions
        let bits = bs.into_bytes();
        let n = crate::alphabet::biguint_from_bytes_be(&bits);
        let payload = crate::alphabet::to_base(&n, BASE85_ALPHABET);
        match decode(&payload) {
            Err(Error::UnsupportedVersion(3)) => {}
            other => panic!(
                "expected UnsupportedVersion(3), got {:?}",
                other.map(|_| ())
            ),
        }
    }

    #[test]
    fn rejects_wrong_dict_set() {
        let mut bs = crate::bitstream::WriteBitStream::new();
        bs.write_bits(1, 4).unwrap();
        bs.write_bits(9, 4).unwrap(); // wrong dict_set_id
        bs.write_bits(1, 1).unwrap();
        bs.write_bits(0, 1).unwrap();
        bs.write_bits(0, 2).unwrap();
        bs.write_bits(0, 2).unwrap();
        bs.write_bits(0, 8).unwrap();
        bs.write_bits(0, 1).unwrap();
        bs.write_bits(TLD_ESCAPE as u64, 5).unwrap();
        bs.write_bits(0, 3).unwrap();
        let bits = bs.into_bytes();
        let n = crate::alphabet::biguint_from_bytes_be(&bits);
        let payload = crate::alphabet::to_base(&n, BASE85_ALPHABET);
        assert!(matches!(
            decode(&payload),
            Err(Error::InvalidPayload { .. })
        ));
    }

    #[test]
    fn rejects_truncated_payload() {
        let encoded = encode("https://example.com/a/b").unwrap();
        // Truncate the base85 string: from_base still succeeds but the
        // bitstream is cut short, so a read must fail.
        let truncated = &encoded[..encoded.len() / 2];
        assert!(decode(truncated).is_err());
    }

    #[test]
    fn rejects_non_zero_padding() {
        let encoded = encode("https://example.com/").unwrap();
        let n = crate::alphabet::from_base(&encoded, BASE85_ALPHABET).unwrap();
        let mut bits = crate::alphabet::bytes_from_biguint_be(&n);
        // Flip the lowest bit of the last byte: that bit is padding.
        let last = bits.last_mut().unwrap();
        *last ^= 1;
        let n2 = crate::alphabet::biguint_from_bytes_be(&bits);
        let payload = crate::alphabet::to_base(&n2, BASE85_ALPHABET);
        assert!(matches!(
            decode(&payload),
            Err(Error::InvalidPayload { .. })
        ));
    }

    #[test]
    fn rejects_segment_count_over_64() {
        // Craft a bitstream with a path region declaring 65 segments.
        let mut bs = crate::bitstream::WriteBitStream::new();
        bs.write_bits(1, 4).unwrap();
        bs.write_bits(DICT_SET_ID as u64, 4).unwrap();
        bs.write_bits(1, 1).unwrap();
        bs.write_bits(0, 1).unwrap();
        bs.write_bits(0, 2).unwrap();
        bs.write_bits(0, 2).unwrap();
        bs.write_bits(0, 8).unwrap();
        bs.write_bits(0, 1).unwrap();
        bs.write_bits(TLD_ESCAPE as u64, 5).unwrap();
        bs.write_bits(1, 1).unwrap(); // path present
        bs.write_bits(0, 1).unwrap(); // query absent
        bs.write_bits(0, 1).unwrap(); // fragment absent
        bs.write_varint(65).unwrap();
        let bits = bs.into_bytes();
        let n = crate::alphabet::biguint_from_bytes_be(&bits);
        let payload = crate::alphabet::to_base(&n, BASE85_ALPHABET);
        assert!(matches!(
            decode(&payload),
            Err(Error::InvalidPayload { .. })
        ));
    }

    #[test]
    fn rejects_symbol_count_over_4096() {
        // Craft a bitstream with one path segment declaring 4097 symbols.
        let mut bs = crate::bitstream::WriteBitStream::new();
        bs.write_bits(1, 4).unwrap();
        bs.write_bits(DICT_SET_ID as u64, 4).unwrap();
        bs.write_bits(1, 1).unwrap();
        bs.write_bits(0, 1).unwrap();
        bs.write_bits(0, 2).unwrap();
        bs.write_bits(0, 2).unwrap();
        bs.write_bits(0, 8).unwrap();
        bs.write_bits(0, 1).unwrap();
        bs.write_bits(TLD_ESCAPE as u64, 5).unwrap();
        bs.write_bits(1, 1).unwrap();
        bs.write_bits(0, 1).unwrap();
        bs.write_bits(0, 1).unwrap();
        bs.write_varint(1).unwrap(); // one segment
        bs.write_bits(0, 4).unwrap(); // alphabet_id 0
        bs.write_varint(4097).unwrap(); // symbol_count
        let bits = bs.into_bytes();
        let n = crate::alphabet::biguint_from_bytes_be(&bits);
        let payload = crate::alphabet::to_base(&n, BASE85_ALPHABET);
        assert!(matches!(
            decode(&payload),
            Err(Error::InvalidPayload { .. })
        ));
    }

    #[test]
    fn rejects_garbage_base85() {
        assert!(decode("!!!!not-a-valid-payload!!!!").is_err());
    }

    #[test]
    fn rejects_reserved_host_mode() {
        let mut bs = crate::bitstream::WriteBitStream::new();
        bs.write_bits(1, 4).unwrap();
        bs.write_bits(DICT_SET_ID as u64, 4).unwrap();
        bs.write_bits(1, 1).unwrap();
        bs.write_bits(0, 1).unwrap();
        bs.write_bits(0, 2).unwrap();
        bs.write_bits(3, 2).unwrap(); // reserved host mode
        let bits = bs.into_bytes();
        let n = crate::alphabet::biguint_from_bytes_be(&bits);
        let payload = crate::alphabet::to_base(&n, BASE85_ALPHABET);
        assert!(matches!(
            decode(&payload),
            Err(Error::InvalidPayload { .. })
        ));
    }

    #[test]
    fn rejects_host_index_out_of_range() {
        // host_mode 0 with a dictionary index ≥ 40 must be rejected, not
        // crash: `host_at` panics out of bounds and decoding must
        // never panic on malformed payloads.
        for idx in [40u8, 114, 254] {
            let mut bs = crate::bitstream::WriteBitStream::new();
            bs.write_bits(1, 4).unwrap();
            bs.write_bits(DICT_SET_ID as u64, 4).unwrap();
            bs.write_bits(1, 1).unwrap();
            bs.write_bits(0, 1).unwrap();
            bs.write_bits(0, 2).unwrap();
            bs.write_bits(0, 2).unwrap(); // host mode 0
            bs.write_bits(idx as u64, 8).unwrap();
            bs.write_bits(0, 1).unwrap(); // tld mode 0
            bs.write_bits(TLD_ESCAPE as u64, 5).unwrap();
            bs.write_bits(0, 3).unwrap(); // no resource regions
            let bits = bs.into_bytes();
            let n = crate::alphabet::biguint_from_bytes_be(&bits);
            let payload = crate::alphabet::to_base(&n, BASE85_ALPHABET);
            assert!(
                matches!(decode(&payload), Err(Error::InvalidPayload { .. })),
                "host index {idx} should be rejected"
            );
        }
    }

    #[test]
    fn decodes_host_escape_literal() {
        // Host mode 0 with index 255 (HOST_ESCAPE) followed by a literal
        // lowercase segment.
        let mut bs = crate::bitstream::WriteBitStream::new();
        bs.write_bits(1, 4).unwrap();
        bs.write_bits(DICT_SET_ID as u64, 4).unwrap();
        bs.write_bits(1, 1).unwrap();
        bs.write_bits(0, 1).unwrap();
        bs.write_bits(0, 2).unwrap();
        bs.write_bits(0, 2).unwrap(); // host mode 0
        bs.write_bits(HOST_ESCAPE as u64, 8).unwrap();
        // literal segment "abc" (base26-lower, 3 symbols, value 28, 5 bits)
        bs.write_bits(1, 4).unwrap();
        bs.write_varint(3).unwrap();
        bs.write_varint(5).unwrap();
        bs.write_bits(28, 5).unwrap();
        bs.write_bits(0, 1).unwrap(); // tld mode 0
        bs.write_bits(TLD_ESCAPE as u64, 5).unwrap();
        bs.write_bits(0, 3).unwrap();
        let bits = bs.into_bytes();
        let n = crate::alphabet::biguint_from_bytes_be(&bits);
        let payload = crate::alphabet::to_base(&n, BASE85_ALPHABET);
        assert_eq!(decode(&payload).unwrap(), "https://abc");
    }

    #[test]
    fn decodes_huffman_forward_compat_segment() {
        // alphabet_id 5 is a huffman-compressed segment. The
        // canonical-first symbol's code is all zeros; encoding two of them
        // yields an all-zero stream whose leading zero bits must survive
        // decoding (regression: BigUint conversion used to drop them,
        // failing to fill the huffman decoder buffer). Derived from the
        // default codebook so this stays valid across corpus retrains.
        let codebook = crate::huffman::default_codebook();
        let first_sym = codebook
            .0
            .iter()
            .enumerate()
            .filter(|&(_, &len)| len > 0)
            .min_by_key(|&(sym, len)| (len, sym))
            .map(|(sym, _)| sym as u8)
            .unwrap();
        let slen = codebook.0[first_sym as usize] as usize;
        let word = [first_sym, first_sym];
        let encoder = crate::huffman::HuffmanEncoder::new(&codebook).unwrap();
        let (hbits, _) = encoder.encode(&word).unwrap();
        let zero_bits = slen * 2;
        assert!(hbits.iter().all(|&b| b == 0) || !zero_bits.is_multiple_of(8));
        let hlen = slen * 2;

        let mut bs = crate::bitstream::WriteBitStream::new();
        bs.write_bits(1, 4).unwrap();
        bs.write_bits(DICT_SET_ID as u64, 4).unwrap();
        bs.write_bits(1, 1).unwrap();
        bs.write_bits(0, 1).unwrap();
        bs.write_bits(0, 2).unwrap();
        bs.write_bits(1, 2).unwrap(); // host mode 1 (literal lowercase)
        bs.write_bits(5, 4).unwrap(); // alphabet_id 5 (huffman)
        bs.write_varint(2).unwrap(); // symbol_count
        bs.write_varint(hlen as u64).unwrap(); // value_bit_length
        bs.write_bits(0, hlen as u32).unwrap(); // canonical-first codes are zeros
        bs.write_bits(0, 1).unwrap(); // tld mode 0
        bs.write_bits(TLD_ESCAPE as u64, 5).unwrap();
        bs.write_bits(0, 3).unwrap(); // no resource regions
        let bits = bs.into_bytes();
        let n = crate::alphabet::biguint_from_bytes_be(&bits);
        let payload = crate::alphabet::to_base(&n, BASE85_ALPHABET);
        let expected = format!("https://{}{}", first_sym as char, first_sym as char);
        assert_eq!(decode(&payload).unwrap(), expected);
    }
}
