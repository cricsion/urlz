# urlz

[![Crates.io](https://img.shields.io/crates/v/urlz.svg)](https://crates.io/crates/urlz)
[![Docs.rs](https://docs.rs/urlz/badge.svg)](https://docs.rs/urlz)
[![License](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](LICENSE-MIT)
[![CI](https://github.com/cricsion/urlz/actions/workflows/ci.yml/badge.svg)](https://github.com/cricsion/urlz/actions/workflows/ci.yml)

High-efficiency URL compression in Rust.

urlz rewrites URLs into compact payloads by exploiting their structure: known
hosts and TLDs become dictionary indices, boilerplate like `index.html` suffixes
is elided, path/query segments are encoded in whichever of several alphabets is
smallest, and a canonical Huffman code is applied per segment when it wins on
wire size. The result is emitted as compact base85 text.

**Not guaranteed shorter.** Some payloads can exceed their source — high-entropy
or irregular raw byte sequences can expand slightly due to framing overhead.
Structured URLs are where it pays off.

## Quick start

Requires Rust 1.98+ (edition 2024).

```sh
cargo install urlz                   # from crates.io
cargo install --path crates/urlz     # or from a source checkout
```

Encode and decode:

```sh
$ urlz encode https://example.com/index.html
HOJfBHs-|5:

$ urlz decode 'HOJfBHs-|5:'
https://example.com/index.html
```

Build a Huffman codebook from a URL corpus (one URL per line), or omit the
corpus argument to use the bundled example corpus:

```sh
urlz dict build urls.txt --out ./my_dict
```

## How it works

The pipeline (`crates/urlz/src`):

1. **Parse** (`urlparse`) — split a URL into scheme flags, host/TLD, path
   segments, key/value query pairs, fragments, and `index.*` suffixes.
2. **Segment** (`segment`) — choose an encoding per segment: dictionary hit,
   one of eight segment modes (five radix alphabets, Huffman, raw bytes, or
   percent-encoded bytes) — whichever produces fewer bits including varint overhead.
3. **Huffman** (`huffman`, `dict`) — URL payloads identify the dictionary set
   with a header field and use the default codebook embedded in the binary.
   `urlz dict build` writes a separate codebook file; URL encode/decode do not
   load custom codebooks.
4. **Bitstream** (`bitstream`) — MSB-first bit writer/reader with varints;
   reads never panic on truncated input.
5. **Frame** (`alphabet`) — serialize the whole payload as one big integer in
   base85.

`ARCHITECTURE.md` is the authoritative bit-layout contract and the single source of
truth for all of the above.

## Compression results

Measured with `cargo bench --quick`; full details and macro-benchmarks in [BENCH.md](BENCH.md).

| URL | source | base85 | ratio |
|---|---|---|---|
| `https://example.com/index.html` | 30 | 11 | **2.73×** |
| `https://github.com/rust-lang/rust` | 33 | 16 | **2.06×** |
| `https://example.com` | 19 | 11 | **1.73×** |
| `https://www.google.com/search?q=hello+world` | 43 | 25 | **1.72×** |
| `https://example.com/search?q=rust+url+compression&page=2&sort=desc&filter=all` | 77 | 50 | **1.54×** |

### Macro-Benchmarks (Tranco 1M and Top 10M Datasets)

Tested on an Apple M3 MacBook Air across 14 diverse web archetypes (UUIDs, Git hashes, file extensions, REST APIs, e-commerce, tracking tags, media links, and queries):

| Metric | **Tranco 1 Million (1M URLs)** | **Top 10 Million (10M URLs)** |
|---|:---:|:---:|
| **Unique Domains** | 889,388 | **8,743,106** |
| **Total URLs Encoded** | 1,000,000 | **10,000,000** |
| **Encode Errors** | **0 (100% lossless)** | **0 (100% lossless)** |
| **Throughput** | **447,974 URLs/s** | **450,728 URLs/s** |
| **Mean Latency** | **2.23 µs/URL** | **2.22 µs/URL** |
| **P50 / P90 / P99 Latency** | **1 / 2 / 3 µs** | **1 / 2 / 3 µs** |
| **Total Source Size** | 85.65 MB (85,651,178 chars) | **869.98 MB (869,977,173 chars)** |
| **Total Encoded Size** | 55.35 MB (55,348,350 chars) | **567.44 MB (567,443,631 chars)** |
| **Net Storage Saved** | **30.30 MB saved** | **302.53 MB saved** |
| **Overall Compression Ratio** | **1.547× (35.4% smaller)** | **1.533× (34.8% smaller)** |
| **Path / Media URLs** | **1.573× (36.4% smaller)** | **1.552× (35.6% smaller)** |
| **Query-Heavy URLs** | **1.534× (34.8% smaller)** | **1.524× (34.4% smaller)** |

## Robustness

The decoder treats every input as hostile: varint overflow past `u64`,
over-long groups, out-of-range dictionary indices, unknown format versions,
and non-zero trailing padding are all rejected as errors rather than panics.
This is exercised by property tests (`proptest`), an integration suite, and
adversarial benchmark payloads (64KB garbage, invalid alphabets) — see
`decode_adversarial` in [BENCH.md](BENCH.md).

## Library usage

```rust
use urlz::{decode, encode};

fn main() -> Result<(), urlz::Error> {
    let payload = encode("https://github.com/rust-lang/rust")?;
    let url = decode(&payload)?;
    assert_eq!(url, "https://github.com/rust-lang/rust");
    Ok(())
}
```


## Development

```sh
cargo test                 # unit + integration suites
cargo test -p urlz         # library only
cargo bench -p urlz        # criterion benchmarks (--quick for smoke run)
cargo clippy --all-targets
```

Workspace layout:

```
crates/urlz       # `urlz` codec library and CLI binary
crates/xtask      # dataset and corpus benchmark harnesses
```

Further reading: [USAGE.md](USAGE.md) (user guide & recipes),
[ARCHITECTURE.md](ARCHITECTURE.md) (wire specification & system architecture deep dive),
[BENCH.md](BENCH.md) (benchmarks), [CHANGELOG.md](CHANGELOG.md).

## License

Dual-licensed under [MIT](LICENSE-MIT) or [Apache 2.0](LICENSE-APACHE).
