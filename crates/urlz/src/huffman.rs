//! Canonical Huffman coding over the 85-character base85 alphabet.
//!
//! A [`Codebook`] stores one code length per byte value (0 = unused). Code
//! lengths are produced by [`build_codebook`] from symbol frequencies using a
//! classic Huffman tree, then assigned canonically so that the
//! decoder needs only per-length tables. Codebooks serialize as 256 raw length
//! bytes; [`default_codebook`] embeds the shipped `assets/codebook.bin`.

use std::cmp::Ordering;
use std::collections::BinaryHeap;

use crate::alphabet::{BASE85_ALPHABET, char_index};
use crate::bitstream::{ReadBitStream, WriteBitStream};
use crate::error::Error;

use std::ops::Deref;
use std::sync::LazyLock;

/// Maximum code length in bits.
pub const MAX_CODE_LENGTH: u8 = 64;

/// A Huffman codebook: one code length per byte value, 0 meaning "no code".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Codebook(pub [u8; 256]);

impl Default for Codebook {
    #[inline]
    fn default() -> Self {
        default_codebook()
    }
}

impl AsRef<[u8; 256]> for Codebook {
    #[inline]
    fn as_ref(&self) -> &[u8; 256] {
        &self.0
    }
}

impl Deref for Codebook {
    type Target = [u8; 256];

    #[inline]
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl From<[u8; 256]> for Codebook {
    #[inline]
    fn from(arr: [u8; 256]) -> Self {
        Self(arr)
    }
}

/// Builds a Huffman codebook from symbol frequencies.
///
/// Uses a classic Huffman tree over a min-heap with a deterministic tie-break
/// on `(frequency, id)`: leaf ids are the symbol byte values and internal node
/// ids are assigned in creation order starting at 256. Code lengths are the
/// leaf depths. An all-zero frequency table yields all-zero lengths; a single
/// non-zero symbol gets length 1.
pub fn build_codebook(frequencies: &[u64; 256]) -> Codebook {
    // Node arena: (freq, id, left, right). Leaves have id == symbol and no
    // children; internal nodes have id >= 256 and two children.
    let mut nodes: Vec<(u64, u64, Option<usize>, Option<usize>)> = Vec::new();
    let mut heap: BinaryHeap<HeapNode> = BinaryHeap::new();
    let mut next_id: u64 = 256;
    for (sym, &freq) in frequencies.iter().enumerate() {
        if freq > 0 {
            let index = nodes.len();
            nodes.push((freq, sym as u64, None, None));
            heap.push(HeapNode {
                freq,
                id: sym as u64,
                index,
            });
        }
    }

    let mut lengths = [0u8; 256];
    // Empty or single-symbol input: no tree, or a single length-1 code.
    if heap.len() <= 1 {
        if let Some(node) = heap.pop() {
            lengths[node.id as usize] = 1;
        }
        return Codebook(lengths);
    }

    // Repeatedly merge the two lowest-frequency nodes.
    while heap.len() >= 2 {
        let left = heap.pop().unwrap();
        let right = heap.pop().unwrap();
        let index = nodes.len();
        nodes.push((
            left.freq + right.freq,
            next_id,
            Some(left.index),
            Some(right.index),
        ));
        heap.push(HeapNode {
            freq: left.freq + right.freq,
            id: next_id,
            index,
        });
        next_id += 1;
    }

    // The last remaining node is the root.
    let root = heap.pop().unwrap();

    // DFS from the root, recording leaf depths as code lengths.
    let mut stack = vec![(root.index, 0u8)];
    while let Some((index, depth)) = stack.pop() {
        let (_, sym, left, right) = nodes[index];
        if let (Some(l), Some(r)) = (left, right) {
            stack.push((l, depth + 1));
            stack.push((r, depth + 1));
        } else {
            lengths[sym as usize] = depth;
        }
    }
    Codebook(lengths)
}

/// Serializes a codebook as 256 raw length bytes.
pub fn serialize_codebook(cb: &Codebook) -> Vec<u8> {
    cb.0.to_vec()
}

/// Deserializes a codebook from 256 raw length bytes.
///
/// Rejects wrong lengths, code lengths above [`MAX_CODE_LENGTH`], and tables
/// that violate the Kraft inequality (exact `u128` check).
pub fn deserialize_codebook(bytes: &[u8]) -> Result<Codebook, Error> {
    if bytes.len() != 256 {
        return Err(Error::HuffmanError {
            reason: format!("codebook must be 256 bytes, got {}", bytes.len()),
        });
    }
    let mut lengths = [0u8; 256];
    lengths.copy_from_slice(bytes);
    validate_lengths(&lengths)?;
    Ok(Codebook(lengths))
}

/// Validates code lengths: each at most [`MAX_CODE_LENGTH`] and the Kraft
/// inequality `sum(1 << (64 - len)) <= 1 << 64` (exact `u128` arithmetic).
fn validate_lengths(lengths: &[u8; 256]) -> Result<(), Error> {
    for &len in lengths.iter() {
        if len > MAX_CODE_LENGTH {
            return Err(Error::HuffmanError {
                reason: format!("code length {len} exceeds MAX_CODE_LENGTH"),
            });
        }
    }
    let mut kraft: u128 = 0;
    for &len in lengths.iter() {
        if len > 0 {
            kraft += 1u128 << (64 - u32::from(len));
        }
    }
    if kraft > (1u128 << 64) {
        return Err(Error::HuffmanError {
            reason: "codebook violates the Kraft inequality".to_string(),
        });
    }
    Ok(())
}

/// Canonical tables derived from a codebook.
struct CanonicalTables {
    codes: [u64; 256],
    lengths: [u8; 256],
    sorted: Vec<u8>,
    first_code: [u64; 65],
    first_index: [usize; 65],
    count: [usize; 65],
}

/// Validates a codebook and assigns canonical codes.
///
/// Symbols are sorted by `(length, symbol)`; the first code is 0 and each
/// subsequent code is `(prev + 1) << (curr_len - prev_len)`. Per-length
/// `first_code`/`first_index`/`count` tables are precomputed for decoding.
fn build_canonical_tables(cb: &Codebook) -> Result<CanonicalTables, Error> {
    let lengths = cb.0;
    validate_lengths(&lengths)?;

    let mut symbols: Vec<u8> = (0..=255u8).filter(|&s| lengths[s as usize] > 0).collect();
    symbols.sort_by_key(|&s| (lengths[s as usize], s));

    let mut codes = [0u64; 256];
    let mut first_code = [0u64; 65];
    let mut first_index = [0usize; 65];
    let mut count = [0usize; 65];
    let mut code: u64 = 0;
    let mut prev_len: u8 = 0;
    for (i, &sym) in symbols.iter().enumerate() {
        let len = lengths[sym as usize];
        code = if i == 0 {
            0
        } else {
            (code + 1) << (len - prev_len)
        };
        codes[sym as usize] = code;
        if count[len as usize] == 0 {
            first_code[len as usize] = code;
            first_index[len as usize] = i;
        }
        count[len as usize] += 1;
        prev_len = len;
    }

    Ok(CanonicalTables {
        codes,
        lengths,
        sorted: symbols,
        first_code,
        first_index,
        count,
    })
}

/// Shipped default Huffman encoder (cached statically).
pub static DEFAULT_HUFFMAN_ENCODER: LazyLock<HuffmanEncoder> =
    LazyLock::new(|| HuffmanEncoder::new(&default_codebook()).expect("default codebook is valid"));

/// Shipped default Huffman decoder (cached statically).
pub static DEFAULT_HUFFMAN_DECODER: LazyLock<HuffmanDecoder> =
    LazyLock::new(|| HuffmanDecoder::new(&default_codebook()).expect("default codebook is valid"));

/// Huffman encoder over a fixed codebook.
#[derive(Debug, Clone)]
pub struct HuffmanEncoder {
    codes: [u64; 256],
    lengths: [u8; 256],
}

impl HuffmanEncoder {
    /// Builds an encoder from a codebook, validating it via the canonical
    /// table construction.
    pub fn new(cb: &Codebook) -> Result<Self, Error> {
        let tables = build_canonical_tables(cb)?;
        Ok(Self {
            codes: tables.codes,
            lengths: tables.lengths,
        })
    }

    /// Encodes `symbols`, returning the packed bytes and the symbol count.
    ///
    /// Rejects symbols outside [`BASE85_ALPHABET`] and symbols with no code.
    pub fn encode(&self, symbols: &[u8]) -> Result<(Vec<u8>, usize), Error> {
        let mut writer = WriteBitStream::new();
        for &sym in symbols {
            if char_index(sym, BASE85_ALPHABET).is_none() {
                return Err(Error::HuffmanError {
                    reason: format!("symbol 0x{sym:02X} is not in the base85 alphabet"),
                });
            }
            let len = self.lengths[sym as usize];
            if len == 0 {
                return Err(Error::HuffmanError {
                    reason: format!("symbol 0x{sym:02X} has no code in this codebook"),
                });
            }
            writer.write_bits(self.codes[sym as usize], u32::from(len))?;
        }
        let bytes = writer.into_bytes();
        Ok((bytes, symbols.len()))
    }

    /// Returns the total number of bits [`Self::encode`] would write for
    /// `symbols`.
    ///
    /// Validates every symbol exactly like [`Self::encode`] but writes nothing.
    #[inline]
    pub fn bit_len(&self, symbols: &[u8]) -> Result<usize, Error> {
        let mut total = 0usize;
        for &sym in symbols {
            if char_index(sym, BASE85_ALPHABET).is_none() {
                return Err(Error::HuffmanError {
                    reason: format!("symbol 0x{sym:02X} is not in the base85 alphabet"),
                });
            }
            let len = self.lengths[sym as usize];
            if len == 0 {
                return Err(Error::HuffmanError {
                    reason: format!("symbol 0x{sym:02X} has no code in this codebook"),
                });
            }
            total += usize::from(len);
        }
        Ok(total)
    }

    /// Encodes `symbols` into `bs`, returning the total bit length written.
    ///
    /// Two-pass: validates every symbol first (same checks as [`Self::encode`]),
    /// so no partial write ever happens on validation failure.
    #[inline]
    pub fn encode_into(&self, bs: &mut WriteBitStream, symbols: &[u8]) -> Result<usize, Error> {
        let total = self.bit_len(symbols)?;
        for &sym in symbols {
            bs.write_bits(
                self.codes[sym as usize],
                u32::from(self.lengths[sym as usize]),
            )?;
        }
        Ok(total)
    }
}

/// Huffman decoder over a fixed codebook.
#[derive(Debug, Clone)]
pub struct HuffmanDecoder {
    sorted: Vec<u8>,
    first_code: [u64; 65],
    first_index: [usize; 65],
    count: [usize; 65],
}

impl HuffmanDecoder {
    /// Builds a decoder from a codebook, validating it via the canonical
    /// table construction.
    pub fn new(cb: &Codebook) -> Result<Self, Error> {
        let tables = build_canonical_tables(cb)?;
        Ok(Self {
            sorted: tables.sorted,
            first_code: tables.first_code,
            first_index: tables.first_index,
            count: tables.count,
        })
    }

    /// Decodes exactly `symbol_count` codes from `bits`.
    ///
    /// Reads codes bit-by-bit, matching each accumulated value against the
    /// per-length canonical tables. After the last symbol the remaining bits
    /// must all be zero (padding); decoded symbols must be in
    /// [`BASE85_ALPHABET`].
    pub fn decode(&self, bits: &[u8], symbol_count: usize) -> Result<Vec<u8>, Error> {
        let mut reader = ReadBitStream::from_bytes(bits);
        let mut out = Vec::with_capacity(symbol_count);
        for _ in 0..symbol_count {
            let mut code: u64 = 0;
            let mut matched = false;
            let mut sym = 0u8;
            for len in 1..=MAX_CODE_LENGTH as usize {
                let bit = reader.read_bits(1).map_err(|_| Error::HuffmanError {
                    reason: "truncated huffman bitstream".to_string(),
                })?;
                code = (code << 1) | bit;
                if self.count[len] > 0 {
                    let first = self.first_code[len];
                    let last = first + self.count[len] as u64;
                    if (first..last).contains(&code) {
                        let idx = self.first_index[len] + (code - first) as usize;
                        sym = self.sorted[idx];
                        matched = true;
                        break;
                    }
                }
            }
            if !matched {
                return Err(Error::HuffmanError {
                    reason: "invalid huffman code in bitstream".to_string(),
                });
            }
            if char_index(sym, BASE85_ALPHABET).is_none() {
                return Err(Error::HuffmanError {
                    reason: format!("decoded symbol 0x{sym:02X} is not in the base85 alphabet"),
                });
            }
            out.push(sym);
        }
        if !reader.read_remaining_all_zero() {
            return Err(Error::HuffmanError {
                reason: "non-zero padding after huffman payload".to_string(),
            });
        }
        Ok(out)
    }
}

/// The shipped default codebook, embedded from `assets/codebook.bin`.
pub fn default_codebook() -> Codebook {
    Codebook(*include_bytes!("../assets/codebook.bin"))
}

/// Compact example corpus (CLI demos and unit tests). The canonical
/// training corpus lives in `assets/corpus.txt` and is not compiled into
/// binaries; see the format specification for codebook provenance.
///
/// One URL per line. Covers every character of [`BASE85_ALPHABET`], so a
/// codebook built from it assigns a code to all 85 symbols.
pub const EXAMPLE_CORPUS: &str = "https://example.com/
https://github.com/rust-lang/rust
https://stackoverflow.com/questions/12345678/how-to-compress
https://en.wikipedia.org/wiki/Huffman_coding
https://www.amazon.com/dp/B08N5WRWNW
https://www.youtube.com/watch?v=abc123XYZ
https://twitter.com/elonmusk/status/1234567890123456789
https://www.reddit.com/r/rust/comments/abc123/hello_world/
https://www.linkedin.com/in/john-doe-123456/
https://medium.com/@someuser/how-to-compress-urls
https://netflix.com/title/80057281
https://www.apple.com/iphone/
https://microsoft.com/en-us/windows
https://web.whatsapp.com/
https://discord.com/channels/123456789/987654321
https://open.spotify.com/track/4cOdK2wGLETKBW3PvgPWqT
https://drive.google.com/file/d/1ABCDEFGHIJKLMNOPQRSTUVWXYZ/view
https://maps.google.com/maps?q=San+Francisco
https://news.ycombinator.com/item?id=12345678
https://play.google.com/store/apps/details?id=com.example.app
https://shop.example.com/products/12345
https://support.example.com/hc/en-us/articles/123456789
https://docs.rs/bitstream-io/latest/bitstream_io/
https://example.com/!important
https://example.com/page#section-1
https://example.com/?price=$100&discount=5
https://example.com/(category)/item
https://example.com/?q=rust*
https://example.com/search/*.html
https://example.com/a,b,c
https://example.com:8080/path:sub
https://example.com/path;param=1
https://example.com/@username
https://example.com/[id]
https://example.com/?q=foo^bar
https://example.com/path`tick
https://example.com/api/{version}
https://example.com/?q=a|b
https://example.com/~username/profile
https://www.google.com/search?q=rust+url+compression&oq=rust+url+compression
https://www.google.com/search?q=how+to+compress+urls&num=20&safe=active
https://www.bing.com/search?q=url+shortener&form=QBLH
https://duckduckgo.com/?q=huffman+coding&t=ffab
https://www.google.com/search?q=query+string+parameters&page=2&sort=by-date
https://example.com/search?q=compress&filters=all&type=web
https://example.com/products?category=shoes&brand=acme&sort=price-asc&page=3
https://example.com/articles?page=4&limit=25&order=newest
https://example.com/list?offset=50&count=10&ref=sidebar
https://example.com/api/v1/users/12345/posts?limit=20&offset=40
https://example.com/api/v2/items?ids=1,2,3&fields=name,price
https://example.com/checkout?item=98765&qty=2&coupon=SAVE10
https://example.com/?utm_source=newsletter&utm_medium=email&utm_campaign=spring
https://example.com/landing?utm_source=twitter&utm_medium=social&utm_content=bio
https://example.com/watch?v=dQw4w9WgXcQ&list=PL1234567890&t=42
https://video.example.com/play?id=99887766&autoplay=true&muted=false
https://mail.example.com/inbox?folder=archive&q=meeting+notes
https://forum.example.com/thread/98765?page=7&sort=votes#reply-form
https://blog.example.com/2024/how-url-compression-works
https://news.example.com/tech/article-title-goes-here?ref=rss
https://wiki.example.org/index.php?title=Canonical_Huffman&action=edit
https://en.wikipedia.org/wiki/Query_string?useskin=vector-2022
https://example.com/docs/guide/getting-started.html
https://example.com/downloads/releases/latest/app-setup.exe
https://example.com/users/jane-doe/settings/notifications
https://example.com/category/electronics/tvs?price_min=200&price_max=800
https://example.com/tags/rust/compression/feed
https://example.com/profile/edit?tab=privacy&section=visibility
https://translate.example.com/?text=hello+world&from=en&to=de
https://maps.example.com/dir/Current+Location/Coffee?q=coffee+near+me
https://shop.example.com/cart?add=SKU-12345&redirect=/view/basket
https://auth.example.com/login?redirect=https%3A%2F%2Fexample.com%2Fwelcome
https://example.com/callback?code=abc123def456&state=xyz789&scope=read
https://example.com/page?view=mobile&theme=dark&lang=en-US&region=US
https://status.example.com/incidents?since=2024-01-01&until=2024-06-30
https://analytics.example.com/report?start=20240101&end=20241231&granularity=day
https://cloud.example.com/share/f/AbCdEfGhIjKlMnOpQrStUvWx?dl=1";

/// Strips the scheme and authority from a one-URL-per-line corpus, keeping
/// only the path/query/fragment portion of each line.
///
/// For each line, everything through the end of the authority (the first `/`
/// after `://`) is dropped; the remainder (path, query, fragment) is kept.
/// Lines without `://` are treated as bare resources and kept whole. Blank
/// lines are skipped. Lines are processed in input order, so the result is
/// deterministic.
pub fn parse_corpus(text: &str) -> String {
    let mut out = String::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let resource = match line.find("://") {
            Some(scheme_end) => {
                let rest = &line[scheme_end + 3..];
                match rest.find(['/', '?', '#']) {
                    Some(idx) => &rest[idx..],
                    None => "",
                }
            }
            None => line,
        };
        out.push_str(resource);
    }
    out
}

/// Counts [`BASE85_ALPHABET`] symbol frequencies over `text` and builds a
/// [`Codebook`] from them.
///
/// Characters outside the alphabet are ignored. Deterministic: the same
/// corpus always yields the same codebook.
pub fn build_from_corpus(text: &str) -> Codebook {
    let mut freqs = [0u64; 256];
    for &b in text.as_bytes() {
        if char_index(b, BASE85_ALPHABET).is_some() {
            freqs[b as usize] += 1;
        }
    }
    // Baseline smoothing: ensure all 85 valid alphabet characters have non-zero
    // frequency so that any valid base85 character is encodable.
    for &b in BASE85_ALPHABET.iter() {
        if freqs[b as usize] == 0 {
            freqs[b as usize] = 1;
        }
    }
    build_codebook(&freqs)
}

/// Serializes `cb` via [`serialize_codebook`] and writes it to `path` as 256
/// raw length bytes.
pub fn write_codebook_file(path: impl AsRef<std::path::Path>, cb: &Codebook) -> Result<(), Error> {
    std::fs::write(path, serialize_codebook(cb))?;
    Ok(())
}

/// A min-heap entry ordered by `(frequency, id)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct HeapNode {
    freq: u64,
    id: u64,
    index: usize,
}

impl Ord for HeapNode {
    fn cmp(&self, other: &Self) -> Ordering {
        other
            .freq
            .cmp(&self.freq)
            .then_with(|| other.id.cmp(&self.id))
    }
}

impl PartialOrd for HeapNode {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_all_alphabet_chars() {
        let cb = default_codebook();
        let enc = HuffmanEncoder::new(&cb).unwrap();
        let dec = HuffmanDecoder::new(&cb).unwrap();
        let symbols = BASE85_ALPHABET.to_vec();
        let (bits, count) = enc.encode(&symbols).unwrap();
        assert_eq!(count, symbols.len());
        assert_eq!(dec.decode(&bits, count).unwrap(), symbols);
    }

    #[test]
    fn roundtrip_mixed_strings() {
        let cb = default_codebook();
        let enc = HuffmanEncoder::new(&cb).unwrap();
        let dec = HuffmanDecoder::new(&cb).unwrap();
        for s in [
            "hello",
            "world123",
            "ABC-xyz_012",
            "~user",
            "!important#section",
            "{}[]|^`",
            "0123456789",
        ] {
            let symbols = s.as_bytes().to_vec();
            let (bits, count) = enc.encode(&symbols).unwrap();
            assert_eq!(
                dec.decode(&bits, count).unwrap(),
                symbols,
                "roundtrip failed for {s}"
            );
        }
    }

    #[test]
    fn roundtrip_long_string() {
        let cb = default_codebook();
        let enc = HuffmanEncoder::new(&cb).unwrap();
        let dec = HuffmanDecoder::new(&cb).unwrap();
        let symbols: Vec<u8> = (0..2048).map(|i| BASE85_ALPHABET[i % 85]).collect();
        let (bits, count) = enc.encode(&symbols).unwrap();
        assert_eq!(count, 2048);
        assert_eq!(dec.decode(&bits, count).unwrap(), symbols);
    }

    #[test]
    fn build_codebook_deterministic() {
        let mut freqs = [0u64; 256];
        for (i, &b) in BASE85_ALPHABET.iter().enumerate() {
            freqs[b as usize] = (i as u64 * 7 + 3) % 100;
        }
        let a = build_codebook(&freqs);
        let b = build_codebook(&freqs);
        assert_eq!(a, b);
    }

    #[test]
    fn build_codebook_tie_break_deterministic() {
        let mut freqs = [0u64; 256];
        for &b in BASE85_ALPHABET.iter() {
            freqs[b as usize] = 1;
        }
        let a = build_codebook(&freqs);
        let b = build_codebook(&freqs);
        assert_eq!(a, b);
    }

    #[test]
    fn codes_are_prefix_free() {
        let cb = default_codebook();
        let enc = HuffmanEncoder::new(&cb).unwrap();
        let codes: Vec<(u64, u8)> = (0..=255u8)
            .filter(|&s| enc.lengths[s as usize] > 0)
            .map(|s| (enc.codes[s as usize], enc.lengths[s as usize]))
            .collect();
        for i in 0..codes.len() {
            for j in 0..codes.len() {
                if i == j {
                    continue;
                }
                let (ci, li) = codes[i];
                let (cj, lj) = codes[j];
                if li <= lj {
                    assert_ne!(
                        cj >> (lj - li),
                        ci,
                        "code {ci:0b} (len {li}) is a prefix of {cj:0b} (len {lj})"
                    );
                }
            }
        }
    }

    #[test]
    fn kraft_inequality_holds() {
        let cb = default_codebook();
        let mut kraft: u128 = 0;
        for &len in cb.0.iter() {
            if len > 0 {
                kraft += 1u128 << (64 - u32::from(len));
            }
        }
        assert!(kraft <= (1u128 << 64));
    }

    #[test]
    fn canonical_code_assignment() {
        let mut lengths = [0u8; 256];
        lengths[b'a' as usize] = 2;
        lengths[b'b' as usize] = 2;
        lengths[b'c' as usize] = 3;
        lengths[b'd' as usize] = 3;
        let cb = Codebook(lengths);
        let enc = HuffmanEncoder::new(&cb).unwrap();
        assert_eq!(enc.codes[b'a' as usize], 0b00);
        assert_eq!(enc.codes[b'b' as usize], 0b01);
        assert_eq!(enc.codes[b'c' as usize], 0b100);
        assert_eq!(enc.codes[b'd' as usize], 0b101);
        let dec = HuffmanDecoder::new(&cb).unwrap();
        assert_eq!(dec.decode(&[0x19, 0x40], 4).unwrap(), b"abcd");
    }

    #[test]
    fn serialize_deserialize_roundtrip() {
        let cb = default_codebook();
        let bytes = serialize_codebook(&cb);
        assert_eq!(bytes.len(), 256);
        assert_eq!(deserialize_codebook(&bytes).unwrap(), cb);
    }

    #[test]
    fn deserialize_rejects_malformed() {
        assert!(deserialize_codebook(&[0u8; 255]).is_err());
        assert!(deserialize_codebook(&[0u8; 257]).is_err());
        let mut kraft_violation = [0u8; 256];
        kraft_violation[0] = 1;
        kraft_violation[1] = 1;
        kraft_violation[2] = 1;
        assert!(deserialize_codebook(&kraft_violation).is_err());
        let mut too_long = [0u8; 256];
        too_long[0] = 65;
        assert!(deserialize_codebook(&too_long).is_err());
    }

    #[test]
    fn encoder_rejects_out_of_alphabet() {
        let cb = default_codebook();
        let enc = HuffmanEncoder::new(&cb).unwrap();
        for s in [&b"a/b"[..], b"a=b", b"a%20b", b"\x00"] {
            assert!(enc.encode(s).is_err(), "should reject {s:?}");
        }
    }

    #[test]
    fn encoder_rejects_no_code_symbol() {
        let mut lengths = [0u8; 256];
        lengths[b'a' as usize] = 1;
        let cb = Codebook(lengths);
        let enc = HuffmanEncoder::new(&cb).unwrap();
        assert!(enc.encode(b"ab").is_err());
        let (bits, count) = enc.encode(b"a").unwrap();
        assert_eq!(count, 1);
        let dec = HuffmanDecoder::new(&cb).unwrap();
        assert_eq!(dec.decode(&bits, count).unwrap(), b"a");
    }

    #[test]
    fn bit_len_sums_code_lengths() {
        let cb = default_codebook();
        let enc = HuffmanEncoder::new(&cb).unwrap();
        // bit_len must equal the sum of per-symbol code lengths in the
        // shipped codebook, whichever book that is.
        for s in [&b"tt"[..], b"html", b"index"] {
            let expect: usize = s.iter().map(|&b| cb.0[b as usize] as usize).sum();
            assert_eq!(enc.bit_len(s).unwrap(), expect);
        }
    }

    #[test]
    fn bit_len_rejects_out_of_alphabet() {
        let cb = default_codebook();
        let enc = HuffmanEncoder::new(&cb).unwrap();
        for s in [&b"a/b"[..], b"a=b", b"%20"] {
            assert!(enc.bit_len(s).is_err(), "should reject {s:?}");
        }
    }

    #[test]
    fn bit_len_rejects_no_code() {
        let mut lengths = [0u8; 256];
        lengths[b'a' as usize] = 1;
        let cb = Codebook(lengths);
        let enc = HuffmanEncoder::new(&cb).unwrap();
        assert!(enc.bit_len(b"ab").is_err());
        assert_eq!(enc.bit_len(b"a").unwrap(), 1);
    }

    #[test]
    fn encode_into_matches_encode_bytes() {
        let cb = default_codebook();
        let enc = HuffmanEncoder::new(&cb).unwrap();
        for syms in [&b"tt"[..], b"html", b"sc", b"index", b"a!b"] {
            let mut bs = WriteBitStream::new();
            let n = enc.encode_into(&mut bs, syms).unwrap();
            assert_eq!(n, enc.bit_len(syms).unwrap());
            assert_eq!(bs.into_bytes(), enc.encode(syms).unwrap().0);
        }
    }

    #[test]
    fn encode_into_returns_exact_bit_len() {
        let cb = default_codebook();
        let enc = HuffmanEncoder::new(&cb).unwrap();
        let mut bs = WriteBitStream::new();
        let n = enc.encode_into(&mut bs, b"html").unwrap();
        assert_eq!(n, enc.bit_len(b"html").unwrap());
        assert_eq!(bs.bit_len(), n);
    }

    #[test]
    fn encode_into_empty() {
        let cb = default_codebook();
        let enc = HuffmanEncoder::new(&cb).unwrap();
        let mut bs = WriteBitStream::new();
        assert_eq!(enc.encode_into(&mut bs, b"").unwrap(), 0);
        assert_eq!(bs.bit_len(), 0);
        assert!(bs.into_bytes().is_empty());
    }

    #[test]
    fn encode_into_writes_into_existing_stream() {
        let cb = default_codebook();
        let enc = HuffmanEncoder::new(&cb).unwrap();
        let mut bs = WriteBitStream::new();
        bs.write_bits(1, 4).unwrap();
        let n = enc.encode_into(&mut bs, b"html").unwrap();
        assert_eq!(bs.bit_len(), 4 + n);
        let bytes = bs.into_bytes();
        assert_eq!(bytes[0] >> 4, 0b0001);
    }

    #[test]
    fn empty_input_roundtrip() {
        let cb = default_codebook();
        let enc = HuffmanEncoder::new(&cb).unwrap();
        let dec = HuffmanDecoder::new(&cb).unwrap();
        let (bits, count) = enc.encode(b"").unwrap();
        assert_eq!(count, 0);
        assert!(bits.is_empty());
        assert_eq!(dec.decode(&bits, 0).unwrap(), b"");
    }

    #[test]
    fn decode_truncated_errors() {
        let cb = default_codebook();
        let enc = HuffmanEncoder::new(&cb).unwrap();
        let dec = HuffmanDecoder::new(&cb).unwrap();
        let symbols: Vec<u8> = (0..50).map(|i| BASE85_ALPHABET[i % 85]).collect();
        let (bits, count) = enc.encode(&symbols).unwrap();
        assert!(bits.len() > 1);
        let truncated = &bits[..bits.len() / 2];
        assert!(dec.decode(truncated, count).is_err());
    }

    #[test]
    fn decode_nonzero_padding_errors() {
        let cb = default_codebook();
        let enc = HuffmanEncoder::new(&cb).unwrap();
        let dec = HuffmanDecoder::new(&cb).unwrap();
        let mut input = b"hello-world-0123456789-ABCDEFGHIJKLMNOP".to_vec();
        loop {
            let (bits, count) = enc.encode(&input).unwrap();
            let mut flipped = bits.clone();
            let last = flipped.len() - 1;
            flipped[last] ^= 0x01;
            if dec.decode(&flipped, count).is_err() {
                return;
            }
            // Append more alphabet chars to change the bit alignment until the
            // last byte's low bit is padding rather than a real code bit.
            input.push(BASE85_ALPHABET[input.len() % 85]);
            assert!(input.len() < 200, "no non-byte-aligned input found");
        }
    }

    #[test]
    fn default_codebook_covers_all_alphabet() {
        let cb = default_codebook();
        for &b in BASE85_ALPHABET.iter() {
            assert!(cb.0[b as usize] > 0, "symbol 0x{b:02X} has no code");
        }
    }

    #[test]
    fn default_codebook_matches_file() {
        let cb = default_codebook();
        assert_eq!(
            serialize_codebook(&cb),
            include_bytes!("../assets/codebook.bin")
        );
    }

    #[test]
    fn default_codebook_roundtrip() {
        let cb = default_codebook();
        let enc = HuffmanEncoder::new(&cb).unwrap();
        let dec = HuffmanDecoder::new(&cb).unwrap();
        let symbols: Vec<u8> = (0..1000).map(|i| BASE85_ALPHABET[i % 85]).collect();
        let (bits, count) = enc.encode(&symbols).unwrap();
        assert_eq!(dec.decode(&bits, count).unwrap(), symbols);
    }

    #[test]
    fn dict_tool_parse_corpus_strips_host() {
        let corpus = "https://example.com/path/to?q=1#frag\nhttp://sub.example.org:8080/a/b\n";
        assert_eq!(parse_corpus(corpus), "/path/to?q=1#frag/a/b");
    }

    #[test]
    fn dict_tool_parse_corpus_edge_cases() {
        assert_eq!(parse_corpus("/bare/path\nrelative"), "/bare/pathrelative");
        assert_eq!(parse_corpus("https://example.com"), "");
        assert_eq!(parse_corpus("https://example.com/"), "/");
        assert_eq!(parse_corpus("https://example.com?q=1"), "?q=1");
        assert_eq!(parse_corpus("https://example.com#sec"), "#sec");
        assert_eq!(parse_corpus(""), "");
        assert_eq!(parse_corpus(" \nhttps://example.com/x\n\n"), "/x");
    }

    #[test]
    fn dict_tool_deterministic() {
        let a = build_from_corpus(EXAMPLE_CORPUS);
        let b = build_from_corpus(EXAMPLE_CORPUS);
        assert_eq!(serialize_codebook(&a), serialize_codebook(&b));
        assert_eq!(parse_corpus(EXAMPLE_CORPUS), parse_corpus(EXAMPLE_CORPUS));
    }

    #[test]
    fn dict_tool_roundtrip() {
        for corpus in [EXAMPLE_CORPUS, "a"] {
            let cb = build_from_corpus(corpus);
            let bytes = serialize_codebook(&cb);
            assert_eq!(bytes.len(), 256);
            let back = deserialize_codebook(&bytes).unwrap();
            assert_eq!(back, cb);
        }
    }

    #[test]
    fn dict_tool_example_corpus_covers_alphabet() {
        for &b in BASE85_ALPHABET.iter() {
            assert!(
                EXAMPLE_CORPUS.as_bytes().contains(&b),
                "EXAMPLE_CORPUS missing symbol 0x{b:02X}"
            );
        }
    }
}
