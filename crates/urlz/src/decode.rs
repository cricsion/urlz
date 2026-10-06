//! urlz URL decoder.
//!
//! [`decode`] accepts a base85 payload and converts it to raw bitstream bytes.
//! [`decode_bits`] parses the canonical wire format bit layout and reconstructs
//! the normalized URL.
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
use crate::encode::WIRE_VERSION;
use crate::error::Error;
use crate::huffman::DEFAULT_HUFFMAN_DECODER;
use crate::segment::{encode_pure_percent, fixed_bit_len};

#[inline]
pub(crate) fn read_compact_count(bs: &mut ReadBitStream) -> Result<usize, Error> {
    let nibble = bs.read_bits(4)? as usize;
    if nibble < 15 {
        Ok(nibble)
    } else {
        let extra = usize::try_from(bs.read_varint()?).map_err(|_| Error::InvalidPayload {
            reason: "compact count varint exceeds pointer width".to_string(),
        })?;
        15usize
            .checked_add(extra)
            .ok_or_else(|| Error::InvalidPayload {
                reason: "compact count overflow".to_string(),
            })
    }
}

/// Maximum accepted payload length in bytes.
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
    if version != WIRE_VERSION {
        return Err(Error::UnsupportedVersion(version));
    }
    let dict_set_id = bs.read_bits(4)? as u8;
    if dict_set_id != DICT_SET_ID {
        return Err(Error::InvalidPayload {
            reason: format!("unknown dict_set_id: {dict_set_id} (expected {DICT_SET_ID})"),
        });
    }

    let https = bs.read_bits(1)? == 1;
    let www = bs.read_bits(1)? == 1;
    let index_code = bs.read_bits(2)? as u8;

    let port_flag = bs.read_bits(1)? == 1;
    let port: Option<u16> = if port_flag {
        let mode = bs.read_bits(2)?;
        match mode {
            0 => Some(8080),
            1 => Some(8443),
            2 => Some(3000),
            3 => Some(bs.read_bits(16)? as u16),
            _ => unreachable!(),
        }
    } else {
        None
    };

    let subdomain_flag = bs.read_bits(1)? == 1;
    let host: String = if subdomain_flag {
        let label_count = (bs.read_bits(3)? as usize) + 2;
        let mut labels = Vec::with_capacity(label_count);
        for _ in 0..label_count {
            let is_dict = bs.read_bits(1)? == 1;
            if is_dict {
                let idx = bs.read_bits(8)? as u8;
                if idx as usize >= COMMON_HOSTS.len() {
                    return Err(Error::InvalidPayload {
                        reason: format!("host dictionary index {idx} out of range"),
                    });
                }
                labels.push(host_at(idx).to_string());
            } else {
                labels.push(read_segment(&mut bs)?);
            }
        }
        labels.join(".")
    } else {
        let host_mode = bs.read_bits(2)? as u8;
        match host_mode {
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
            _ => {
                return Err(Error::InvalidPayload {
                    reason: "invalid host mode".to_string(),
                });
            }
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
            } else if idx < 31 {
                tld_at(idx).to_string()
            } else {
                return Err(Error::InvalidPayload {
                    reason: "invalid TLD index".to_string(),
                });
            }
        }
        1 => read_segment(&mut bs)?,
        _ => unreachable!(),
    };
    if tld.len() > MAX_HOST_TLD_LEN {
        return Err(Error::InvalidPayload {
            reason: "tld too long".to_string(),
        });
    }

    let index_literal = match index_code {
        0 => None,
        1 => Some("index.html".to_string()),
        2 => Some("index.php".to_string()),
        3 => Some(read_segment(&mut bs)?),
        _ => unreachable!(),
    };

    let path_present = bs.read_bits(1)? == 1;
    let query_present = bs.read_bits(1)? == 1;
    let fragment_present = bs.read_bits(1)? == 1;

    let mut result = String::with_capacity(64);
    if https {
        result.push_str("https://");
    } else {
        result.push_str("http://");
    }
    if www {
        result.push_str("www.");
    }
    result.push_str(&host);
    if !tld.is_empty() {
        result.push('.');
        result.push_str(&tld);
    }
    if let Some(p) = port {
        let is_default = (https && p == 443) || (!https && p == 80);
        if !is_default {
            result.push(':');
            result.push_str(&p.to_string());
        }
    }

    if path_present {
        let count = read_compact_count(&mut bs)?;
        if count > MAX_SEGMENT_COUNT as usize {
            return Err(Error::InvalidPayload {
                reason: format!("path segment count {count} exceeds limit"),
            });
        }
        for _ in 0..count {
            result.push('/');
            let is_dict = bs.read_bits(1)? == 1;
            if is_dict {
                let token_idx = bs.read_bits(7)? as u8;
                if token_idx as usize >= COMMON_PATH_TOKENS.len() {
                    return Err(Error::InvalidPayload {
                        reason: format!("path token index {token_idx} out of range"),
                    });
                }
                result.push_str(path_token_at(token_idx));
            } else {
                result.push_str(&read_segment(&mut bs)?);
            }
        }
        if let Some(ref lit) = index_literal {
            result.push('/');
            result.push_str(lit);
        }
    } else if let Some(ref lit) = index_literal {
        result.push('/');
        result.push_str(lit);
    }

    if query_present {
        let count = read_compact_count(&mut bs)?;
        if count > MAX_SEGMENT_COUNT as usize {
            return Err(Error::InvalidPayload {
                reason: format!("query segment count {count} exceeds limit"),
            });
        }
        for i in 0..count {
            if i == 0 {
                result.push('?');
            } else {
                result.push('&');
            }
            let key_is_dict = bs.read_bits(1)? == 1;
            if key_is_dict {
                let idx = bs.read_bits(6)? as u8;
                if idx as usize >= COMMON_QUERY_KEYS.len() {
                    return Err(Error::InvalidPayload {
                        reason: format!("query key index {idx} out of range"),
                    });
                }
                result.push_str(query_key_at(idx));
            } else {
                result.push_str(&read_segment(&mut bs)?);
            }

            let has_val = bs.read_bits(1)? == 1;
            if has_val {
                result.push('=');
                let val_is_dict = bs.read_bits(1)? == 1;
                if val_is_dict {
                    let idx = bs.read_bits(5)? as u8;
                    if idx as usize >= COMMON_QUERY_VALUES.len() {
                        return Err(Error::InvalidPayload {
                            reason: format!("query val index {idx} out of range"),
                        });
                    }
                    result.push_str(query_value_at(idx));
                } else {
                    result.push_str(&read_segment(&mut bs)?);
                }
            }
        }
    }

    if fragment_present {
        let count = read_compact_count(&mut bs)?;
        if count > MAX_SEGMENT_COUNT as usize {
            return Err(Error::InvalidPayload {
                reason: format!("fragment segment count {count} exceeds limit"),
            });
        }
        for i in 0..count {
            if i == 0 {
                result.push('#');
            } else {
                result.push('/');
            }
            result.push_str(&read_segment(&mut bs)?);
        }
    }

    if !bs.read_remaining_all_zero() {
        return Err(Error::InvalidPayload {
            reason: "non-zero padding".to_string(),
        });
    }

    Ok(result)
}

/// Read a single segment from the bitstream.
fn read_segment(bs: &mut ReadBitStream) -> Result<String, Error> {
    let alphabet_id = bs.read_bits(3)? as u8;
    let symbol_count = read_compact_count(bs)?;
    if symbol_count > MAX_SYMBOL_COUNT as usize {
        return Err(Error::InvalidPayload {
            reason: format!("symbol count {symbol_count} too large"),
        });
    }

    if alphabet_id == 7 {
        let mut raw = Vec::with_capacity(symbol_count);
        for _ in 0..symbol_count {
            raw.push(bs.read_bits(8)? as u8);
        }
        return Ok(encode_pure_percent(&raw));
    }

    if alphabet_id == 5 {
        let decoded = DEFAULT_HUFFMAN_DECODER.decode_from(bs, symbol_count)?;
        return String::from_utf8(decoded).map_err(|_| Error::InvalidPayload {
            reason: "huffman segment is not valid UTF-8".to_string(),
        });
    }

    if alphabet_id == 6 {
        let mut raw = Vec::with_capacity(symbol_count);
        for _ in 0..symbol_count {
            raw.push(bs.read_bits(8)? as u8);
        }
        return String::from_utf8(raw).map_err(|_| Error::InvalidPayload {
            reason: "raw segment is not valid UTF-8".to_string(),
        });
    }

    let bit_len = fixed_bit_len(alphabet_id, symbol_count);
    let value = read_biguint_bits(bs, bit_len as u64)?;
    let symbols = biguint_to_symbols(alphabet_id, &value, symbol_count)?;
    String::from_utf8(symbols).map_err(|_| Error::InvalidPayload {
        reason: "decoded segment is not valid UTF-8".to_string(),
    })
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
            } else if digits.len() == symbol_count {
                Ok(digits)
            } else {
                Err(Error::InvalidPayload {
                    reason: "decoded integer exceeds segment symbol capacity".to_string(),
                })
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
            } else if bytes.len() == symbol_count {
                Ok(bytes)
            } else {
                Err(Error::InvalidPayload {
                    reason: "decoded integer exceeds segment symbol capacity".to_string(),
                })
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
        let mut bs = crate::bitstream::WriteBitStream::new();
        bs.write_bits(3, 4).unwrap(); // wrong version
        bs.write_bits(DICT_SET_ID as u64, 4).unwrap();
        bs.write_bits(1, 1).unwrap();
        bs.write_bits(0, 1).unwrap();
        bs.write_bits(0, 2).unwrap();
        bs.write_bits(0, 1).unwrap(); // port_flag = 0
        bs.write_bits(0, 1).unwrap(); // subdomain_flag = 0
        bs.write_bits(0, 2).unwrap(); // host mode 0
        bs.write_bits(0, 8).unwrap(); // host index 0
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
        bs.write_bits(WIRE_VERSION as u64, 4).unwrap();
        bs.write_bits(9, 4).unwrap(); // wrong dict_set_id
        bs.write_bits(1, 1).unwrap();
        bs.write_bits(0, 1).unwrap();
        bs.write_bits(0, 2).unwrap();
        bs.write_bits(0, 1).unwrap(); // port_flag = 0
        bs.write_bits(0, 1).unwrap(); // subdomain_flag = 0
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
        let truncated = &encoded[..encoded.len() / 2];
        assert!(decode(truncated).is_err());
    }

    #[test]
    fn rejects_non_zero_padding() {
        let encoded = encode("https://example.com/").unwrap();
        let n = crate::alphabet::from_base(&encoded, BASE85_ALPHABET).unwrap();
        let mut bits = crate::alphabet::bytes_from_biguint_be(&n);
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
        let mut bs = crate::bitstream::WriteBitStream::new();
        bs.write_bits(WIRE_VERSION as u64, 4).unwrap();
        bs.write_bits(DICT_SET_ID as u64, 4).unwrap();
        bs.write_bits(1, 1).unwrap();
        bs.write_bits(0, 1).unwrap();
        bs.write_bits(0, 2).unwrap();
        bs.write_bits(0, 1).unwrap(); // port_flag = 0
        bs.write_bits(0, 1).unwrap(); // subdomain_flag = 0
        bs.write_bits(0, 2).unwrap();
        bs.write_bits(0, 8).unwrap();
        bs.write_bits(0, 1).unwrap();
        bs.write_bits(TLD_ESCAPE as u64, 5).unwrap();
        bs.write_bits(1, 1).unwrap(); // path present
        bs.write_bits(0, 1).unwrap(); // query absent
        bs.write_bits(0, 1).unwrap(); // fragment absent
        crate::encode::write_compact_count(&mut bs, 65).unwrap();
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
        let mut bs = crate::bitstream::WriteBitStream::new();
        bs.write_bits(WIRE_VERSION as u64, 4).unwrap();
        bs.write_bits(DICT_SET_ID as u64, 4).unwrap();
        bs.write_bits(1, 1).unwrap();
        bs.write_bits(0, 1).unwrap();
        bs.write_bits(0, 2).unwrap();
        bs.write_bits(0, 1).unwrap(); // port_flag = 0
        bs.write_bits(0, 1).unwrap(); // subdomain_flag = 0
        bs.write_bits(0, 2).unwrap();
        bs.write_bits(0, 8).unwrap();
        bs.write_bits(0, 1).unwrap();
        bs.write_bits(TLD_ESCAPE as u64, 5).unwrap();
        bs.write_bits(1, 1).unwrap(); // path present
        bs.write_bits(0, 1).unwrap();
        bs.write_bits(0, 1).unwrap();
        crate::encode::write_compact_count(&mut bs, 1).unwrap();
        bs.write_bits(0, 1).unwrap(); // is_dict = 0
        bs.write_bits(0, 3).unwrap(); // alphabet_id 0
        crate::encode::write_compact_count(&mut bs, 4097).unwrap(); // symbol_count
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
        bs.write_bits(WIRE_VERSION as u64, 4).unwrap();
        bs.write_bits(DICT_SET_ID as u64, 4).unwrap();
        bs.write_bits(1, 1).unwrap();
        bs.write_bits(0, 1).unwrap();
        bs.write_bits(0, 2).unwrap();
        bs.write_bits(0, 1).unwrap(); // port_flag = 0
        bs.write_bits(0, 1).unwrap(); // subdomain_flag = 0
        bs.write_bits(3, 2).unwrap(); // reserved host mode 3
        let bits = bs.into_bytes();
        let n = crate::alphabet::biguint_from_bytes_be(&bits);
        let payload = crate::alphabet::to_base(&n, BASE85_ALPHABET);
        assert!(matches!(
            decode(&payload),
            Err(Error::InvalidPayload { .. })
        ));
    }

    #[test]
    fn decodes_host_escape_literal() {
        let mut bs = crate::bitstream::WriteBitStream::new();
        bs.write_bits(WIRE_VERSION as u64, 4).unwrap();
        bs.write_bits(DICT_SET_ID as u64, 4).unwrap();
        bs.write_bits(1, 1).unwrap();
        bs.write_bits(0, 1).unwrap();
        bs.write_bits(0, 2).unwrap();
        bs.write_bits(0, 1).unwrap(); // port_flag = 0
        bs.write_bits(0, 1).unwrap(); // subdomain_flag = 0
        bs.write_bits(0, 2).unwrap(); // host mode 0
        bs.write_bits(HOST_ESCAPE as u64, 8).unwrap();
        // literal segment "abc"
        crate::encode::write_segment(&mut bs, "abc", false).unwrap();
        bs.write_bits(0, 1).unwrap(); // tld mode 0
        bs.write_bits(TLD_ESCAPE as u64, 5).unwrap();
        bs.write_bits(0, 3).unwrap(); // no resource regions
        let bits = bs.into_bytes();
        let n = crate::alphabet::biguint_from_bytes_be(&bits);
        let payload = crate::alphabet::to_base(&n, BASE85_ALPHABET);
        assert_eq!(decode(&payload).unwrap(), "https://abc");
    }

    #[test]
    fn rejects_subdomain_host_index_out_of_range() {
        let mut bs = crate::bitstream::WriteBitStream::new();
        bs.write_bits(WIRE_VERSION as u64, 4).unwrap();
        bs.write_bits(DICT_SET_ID as u64, 4).unwrap();
        bs.write_bits(1, 1).unwrap();
        bs.write_bits(0, 1).unwrap();
        bs.write_bits(0, 2).unwrap();
        bs.write_bits(0, 1).unwrap(); // port_flag = 0
        bs.write_bits(1, 1).unwrap(); // subdomain_flag = 1
        bs.write_bits(0, 3).unwrap(); // label_count - 2 = 0 -> 2 labels
        bs.write_bits(1, 1).unwrap(); // label 0 is_dict = 1
        bs.write_bits(255, 8).unwrap(); // index 255 is out of range for COMMON_HOSTS (len 255)
        let bits = bs.into_bytes();
        let n = crate::alphabet::biguint_from_bytes_be(&bits);
        let payload = crate::alphabet::to_base(&n, BASE85_ALPHABET);
        assert!(matches!(
            decode(&payload),
            Err(Error::InvalidPayload { .. })
        ));
    }

    #[test]
    fn rejects_compact_count_overflow() {
        let mut bs = crate::bitstream::WriteBitStream::new();
        bs.write_bits(WIRE_VERSION as u64, 4).unwrap();
        bs.write_bits(DICT_SET_ID as u64, 4).unwrap();
        bs.write_bits(1, 1).unwrap();
        bs.write_bits(0, 1).unwrap();
        bs.write_bits(0, 2).unwrap();
        bs.write_bits(0, 1).unwrap(); // port_flag = 0
        bs.write_bits(0, 1).unwrap(); // subdomain_flag = 0
        bs.write_bits(0, 2).unwrap();
        bs.write_bits(0, 8).unwrap();
        bs.write_bits(0, 1).unwrap();
        bs.write_bits(TLD_ESCAPE as u64, 5).unwrap();
        bs.write_bits(1, 1).unwrap(); // path present
        bs.write_bits(0, 1).unwrap();
        bs.write_bits(0, 1).unwrap();
        bs.write_bits(15, 4).unwrap(); // compact nibble 15
        bs.write_varint(u64::MAX).unwrap(); // adversarial varint
        let bits = bs.into_bytes();
        let n = crate::alphabet::biguint_from_bytes_be(&bits);
        let payload = crate::alphabet::to_base(&n, BASE85_ALPHABET);
        assert!(matches!(
            decode(&payload),
            Err(Error::InvalidPayload { .. })
        ));
    }
}
