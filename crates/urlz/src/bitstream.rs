//! Bit-level stream reading and writing.
//!
//! Provides [`WriteBitStream`] and [`ReadBitStream`], thin wrappers around
//! `bitstream_io`'s big-endian (MSB-first) bit writer and reader. All reads
//! and writes are validated and mapped to [`Error`]; reads never panic
//! on truncated input.

use bitstream_io::{BigEndian, BitRead, BitReader, BitWrite, BitWriter};

use crate::error::Error;

use std::fmt;

/// Maximum number of 8-bit groups in a varint (7 data bits each), covering a
/// full `u64`.
const VARINT_MAX_GROUPS: usize = 10;

/// A bit writer over an in-memory byte buffer, MSB-first.
pub struct WriteBitStream {
    writer: BitWriter<Vec<u8>, BigEndian>,
    bit_len: usize,
}

impl fmt::Debug for WriteBitStream {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("WriteBitStream")
            .field("bit_len", &self.bit_len)
            .finish()
    }
}

impl WriteBitStream {
    /// Creates a new empty bit writer with a default pre-allocated capacity.
    #[inline]
    pub fn new() -> Self {
        Self::with_capacity(64)
    }

    /// Creates a new empty bit writer with the specified byte capacity.
    #[inline]
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            writer: BitWriter::new(Vec::with_capacity(capacity)),
            bit_len: 0,
        }
    }

    /// Writes the low `count` bits of `value`, MSB-first.
    ///
    /// Returns [`Error::InvalidPayload`] if `count > 64` or if `value`
    /// does not fit in `count` bits.
    #[inline]
    pub fn write_bits(&mut self, value: u64, count: u32) -> Result<(), Error> {
        if count > 64 {
            return Err(Error::InvalidPayload {
                reason: format!("bit count {count} exceeds 64"),
            });
        }
        if count < 64 && value >= (1u64 << count) {
            return Err(Error::InvalidPayload {
                reason: format!("value {value} does not fit in {count} bits"),
            });
        }
        self.writer
            .write(count, value)
            .map_err(|e| Error::InvalidPayload {
                reason: format!("write_bits failed: {e}"),
            })?;
        self.bit_len += count as usize;
        Ok(())
    }

    /// Writes `value` as a varint: groups of 7 data bits, least-significant
    /// group first, each group written as `[continuation_bit][7 data bits]`
    /// MSB-first. Rejects values requiring more than 10 groups.
    #[inline]
    pub fn write_varint(&mut self, value: u64) -> Result<(), Error> {
        if value == 0 {
            return self.write_bits(0, 8);
        }
        let mut groups = [0u8; VARINT_MAX_GROUPS];
        let mut count = 0;
        let mut v = value;
        while v > 0 {
            if count >= VARINT_MAX_GROUPS {
                return Err(Error::InvalidPayload {
                    reason: "varint value requires more than 10 groups".to_string(),
                });
            }
            groups[count] = (v & 0x7F) as u8;
            v >>= 7;
            count += 1;
        }
        for (i, &g) in groups[..count].iter().enumerate() {
            let cont = if i < count - 1 { 0x80 } else { 0x00 };
            self.write_bits(cont | g as u64, 8)?;
        }
        Ok(())
    }

    /// Flushes remaining bits (zero-padded to a byte boundary) and returns
    /// the written bytes.
    #[inline]
    pub fn into_bytes(self) -> Vec<u8> {
        let mut writer = self.writer;
        // byte_align pads with zero bits; it cannot fail for a Vec<u8> sink.
        let _ = writer.byte_align();
        writer.into_writer()
    }

    /// Returns the number of bits written so far.
    #[inline]
    pub fn bit_len(&self) -> usize {
        self.bit_len
    }
}

impl Default for WriteBitStream {
    #[inline]
    fn default() -> Self {
        Self::new()
    }
}

/// A bit reader over a byte slice, MSB-first.
pub struct ReadBitStream<'a> {
    reader: BitReader<&'a [u8], BigEndian>,
    bytes: &'a [u8],
    bit_pos: usize,
}

impl fmt::Debug for ReadBitStream<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ReadBitStream")
            .field("bytes_len", &self.bytes.len())
            .field("bit_pos", &self.bit_pos)
            .field("remaining_bits", &self.remaining_bits())
            .finish()
    }
}

impl<'a> ReadBitStream<'a> {
    /// Creates a reader over `bytes`.
    #[inline]
    pub fn from_bytes(bytes: &'a [u8]) -> Self {
        Self {
            reader: BitReader::new(bytes),
            bytes,
            bit_pos: 0,
        }
    }

    /// Reads `count` bits, MSB-first.
    ///
    /// Returns [`Error::InvalidPayload`] if `count > 64` or if fewer
    /// than `count` bits remain. Never panics on truncated input.
    #[inline]
    pub fn read_bits(&mut self, count: u32) -> Result<u64, Error> {
        if count > 64 {
            return Err(Error::InvalidPayload {
                reason: format!("bit count {count} exceeds 64"),
            });
        }
        let value: u64 = self.reader.read(count).map_err(|e| Error::InvalidPayload {
            reason: format!("read_bits failed: {e}"),
        })?;
        self.bit_pos += count as usize;
        Ok(value)
    }

    /// Reads a varint. Returns [`Error::InvalidPayload`] on
    /// truncated input, more than 10 groups, or a value that overflows `u64`.
    #[inline]
    pub fn read_varint(&mut self) -> Result<u64, Error> {
        let mut result: u64 = 0;
        let mut shift: u32 = 0;
        for _ in 0..VARINT_MAX_GROUPS {
            let byte = self.read_bits(8)?;
            let data = byte & 0x7F;
            let cont = byte >> 7;
            if shift > 0 && data >= (1u64 << (64 - shift)) {
                return Err(Error::InvalidPayload {
                    reason: "varint value overflows u64".to_string(),
                });
            }
            result |= data << shift;
            shift += 7;
            if cont == 0 {
                return Ok(result);
            }
        }
        Err(Error::InvalidPayload {
            reason: "varint exceeds 10 groups".to_string(),
        })
    }

    /// Returns the number of unread bits.
    #[inline]
    pub fn remaining_bits(&self) -> usize {
        self.bytes.len() * 8 - self.bit_pos
    }

    /// Returns `true` if every unread bit is zero. Does not advance the read
    /// position. Used to validate trailing zero padding.
    #[inline]
    pub fn read_remaining_all_zero(&self) -> bool {
        bits_all_zero_from(self.bytes, self.bit_pos)
    }
}

/// Returns `true` if all bits from `start_bit` (inclusive) to the end of
/// `bytes` are zero.
fn bits_all_zero_from(bytes: &[u8], start_bit: usize) -> bool {
    let total_bits = bytes.len() * 8;
    if start_bit >= total_bits {
        return true;
    }
    let byte_idx = start_bit / 8;
    let bit_off = start_bit % 8;
    if bit_off != 0 {
        let mask = 0xFFu8 >> bit_off;
        if bytes[byte_idx] & mask != 0 {
            return false;
        }
        bytes[byte_idx + 1..].iter().all(|&b| b == 0)
    } else {
        bytes[byte_idx..].iter().all(|&b| b == 0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn varint_roundtrip() {
        for v in [0u64, 1, 127, 128, 16384, u64::MAX] {
            let mut w = WriteBitStream::new();
            w.write_varint(v).unwrap();
            let bytes = w.into_bytes();
            let mut r = ReadBitStream::from_bytes(&bytes);
            assert_eq!(
                r.read_varint().unwrap(),
                v,
                "varint roundtrip failed for {v}"
            );
        }
    }

    #[test]
    fn varint_exact_encoding() {
        // 300 = 0b1_0010_1100 → groups [44, 2] → bytes [0xAC, 0x02].
        let mut w = WriteBitStream::new();
        w.write_varint(300).unwrap();
        assert_eq!(w.into_bytes(), [0xAC, 0x02]);

        // 0 → single group 0 → byte [0x00].
        let mut w = WriteBitStream::new();
        w.write_varint(0).unwrap();
        assert_eq!(w.into_bytes(), [0x00]);
    }

    #[test]
    fn read_past_end_errors() {
        let mut r = ReadBitStream::from_bytes(&[0xFF]);
        assert_eq!(r.read_bits(8).unwrap(), 0xFF);
        assert!(r.read_bits(1).is_err());
        assert!(r.read_bits(8).is_err());

        let mut r = ReadBitStream::from_bytes(&[0xFF; 16]);
        assert!(r.read_bits(65).is_err());

        // read_varint past end (continuation bit set, no more bytes).
        let mut r = ReadBitStream::from_bytes(&[0x80]);
        assert!(r.read_varint().is_err());

        let mut r = ReadBitStream::from_bytes(&[0x80; 10]);
        assert!(r.read_varint().is_err());

        // read_varint whose 10th group overflows u64.
        let mut bytes = vec![0x80u8; 9];
        bytes.push(0x82); // data = 2 at shift 63 → overflow
        let mut r = ReadBitStream::from_bytes(&bytes);
        assert!(r.read_varint().is_err());
    }

    #[test]
    fn bit_len_tracks_writes() {
        let mut w = WriteBitStream::new();
        assert_eq!(w.bit_len(), 0);
        w.write_bits(0b101, 3).unwrap();
        assert_eq!(w.bit_len(), 3);
        w.write_varint(300).unwrap(); // 2 groups = 16 bits
        assert_eq!(w.bit_len(), 3 + 16);
        w.write_bits(0, 1).unwrap();
        assert_eq!(w.bit_len(), 20);
    }

    #[test]
    fn remaining_all_zero() {
        // Zero padding after a 3-bit value.
        let mut w = WriteBitStream::new();
        w.write_bits(0b101, 3).unwrap();
        let bytes = w.into_bytes();
        let mut r = ReadBitStream::from_bytes(&bytes);
        assert_eq!(r.read_bits(3).unwrap(), 0b101);
        assert!(r.read_remaining_all_zero());

        // A set bit remains in the padding.
        let mut w = WriteBitStream::new();
        w.write_bits(0b101, 3).unwrap();
        w.write_bits(1, 1).unwrap();
        let bytes = w.into_bytes();
        let mut r = ReadBitStream::from_bytes(&bytes);
        assert_eq!(r.read_bits(3).unwrap(), 0b101);
        assert!(!r.read_remaining_all_zero());

        // read_remaining_all_zero does not advance the position.
        let mut r = ReadBitStream::from_bytes(&[0xA0]);
        assert_eq!(r.read_bits(3).unwrap(), 0b101);
        assert_eq!(r.remaining_bits(), 5);
        assert!(r.read_remaining_all_zero());
        assert_eq!(r.remaining_bits(), 5);

        let mut r = ReadBitStream::from_bytes(&[0xFF, 0x00]);
        assert_eq!(r.remaining_bits(), 16);
        assert_eq!(r.read_bits(8).unwrap(), 0xFF);
        assert_eq!(r.remaining_bits(), 8);
    }

    #[test]
    fn byte_boundary_padding() {
        let mut w = WriteBitStream::new();
        w.write_bits(0b101, 3).unwrap();
        let bytes = w.into_bytes();
        assert_eq!(bytes.len(), 1);
        assert_eq!(bytes[0], 0b1010_0000); // top 3 bits set + 5 zero padding bits
    }
}
