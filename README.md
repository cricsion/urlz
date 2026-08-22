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

**Not guaranteed shorter.** Some payloads exceed their source — deep paths and
long query strings can expand slightly (e.g. `https://example.com/a/b/c/d/e`
encodes to 30 chars vs 29). Structured URLs are where it pays off.

## Quick start

Requires Rust 1.91+ (edition 2024).

```sh
cargo install urlz-cli        # from crates.io (after publish)
cargo install --path crates/urlz-cli   # or from a source checkout
```

Encode and decode:

```sh
$ urlz encode https://example.com/index.html
<base85 payload>

$ urlz decode <payload>
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
   one of eight fixed alphabets, or Huffman mode — whichever produces fewer
   bits including varint overhead.
3. **Huffman** (`huffman`, `dict`) — canonical codes built from a URL corpus;
   codebooks are serialized alongside the payload contract.
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
| `https://example.com/index.html` | 30 | 14 | **2.14×** |
| `https://www.google.com/search?q=hello+world` | 43 | 28 | **1.54×** |
| `https://example.com` | 19 | 13 | **1.46×** |
| `https://example.com/search?q=rust+url+compression&page=2&...` | 77 | 55 | **1.40×** |
| `https://github.com/rust-lang/rust` | 33 | 24 | **1.38×** |

### Macro-Benchmarks (Tranco 1M and Top 10M Datasets)

Tested across 14 diverse web archetypes (UUIDs, Git hashes, file extensions, REST APIs, e-commerce, tracking tags, media links, and queries):

| Metric | **Tranco 1 Million (1M URLs)** | **Top 10 Million (10M URLs)** |
|---|:---:|:---:|
| **Unique Domains** | 889,388 | **8,743,106** |
| **Total URLs Encoded** | 1,000,000 | **10,000,000** |
| **Encode Errors** | **0 (100% lossless)** | **0 (100% lossless)** |
| **Throughput** | **118,475 URLs/s** | **106,983 URLs/s** |
| **Mean Latency** | **8.44 µs/URL** | **9.35 µs/URL** |
| **P50 / P90 / P99 Latency** | **7 / 15 / 18 µs** | **7 / 16 / 20 µs** |
| **Total Source Size** | 85.65 MB (85,651,178 chars) | **869.98 MB (869,977,173 chars)** |
| **Total Encoded Size** | 70.66 MB (70,658,513 chars) | **719.52 MB (719,521,068 chars)** |
| **Net Storage Saved** | **14.99 MB saved** | **150.46 MB saved** |
| **Overall Compression Ratio** | **1.212× (17.5% smaller)** | **1.209× (17.3% smaller)** |
| **Path / Media URLs** | **1.314× (23.9% smaller)** | **1.306× (23.5% smaller)** |
| **Query-Heavy URLs** | **1.165× (14.2% smaller)** | **1.163× (14.0% smaller)** |

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

let payload = encode("https://github.com/rust-lang/rust")?;
let url = decode(&payload)?;
assert_eq!(url, "https://github.com/rust-lang/rust");
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
crates/urlz       # codec library: parse, segment, huffman, bitstream
crates/urlz-cli   # `urlz` CLI
```

Further reading: [USAGE.md](USAGE.md) (user guide & recipes),
[ARCHITECTURE.md](ARCHITECTURE.md) (wire specification & system architecture deep dive),
[BENCH.md](BENCH.md) (benchmarks), [CHANGELOG.md](CHANGELOG.md).

## License

Dual-licensed under [MIT](LICENSE-MIT) or [Apache 2.0](LICENSE-APACHE).

