# urlz URL Compression — Benchmarks

Criterion benchmarks for `urlz`. Bench groups live in
`crates/urlz/benches/encode_decode.rs`.

## How to run

```sh
cargo bench -p urlz
```

For a fast smoke run (single short measurement per group):

```sh
cargo bench -p urlz --bench encode_decode -- --quick
```

## Bench groups

| Group | Measures |
|---|---|
| `encode` | `encode::encode` (base85) latency per corpus URL |
| `decode` | `decode::decode` (base85) latency per precomputed payload |
| `bytes_per_char` | Compression ratio `src_chars / enc_chars`, computed inside the iter |
| `decode_adversarial` | decode must not panic on near-limit / garbage payloads |

## Corpus

10 URLs covering: dictionary hit, dictionary escape, deep path, long query,
percent-encoded unicode, fragment, non-default port, `index.html` suffix, bare
host, and a search URL.

## Measured results (`cargo bench --quick`)

| URL | source chars | base85 chars | ratio (src/enc) |
|---|---|---|---|
| https://github.com/rust-lang/rust | 33 | 24 | **1.375** |
| https://example-site.com/x | 26 | 24 | **1.083** |
| https://example.com/a/b/c/d/e | 29 | 31 | 0.935 |
| https://example.com/search?q=rust+url+compression&page=2&sort=desc&filter=all | 77 | **55** | **1.400** |
| https://example.com/%E4%B8%AD%E6%96%87/%E8%B7%AF%E5%BE%84 | 57 | 66 | 0.864 |
| https://example.com/page#section-2 | 34 | 30 | **1.133** |
| https://example.com:8080/path | 29 | 31 | 0.935 |
| https://example.com/index.html | 30 | 14 | **2.143** |
| https://example.com | 19 | 13 | **1.462** |
| https://www.google.com/search?q=hello+world | 43 | **28** | **1.536** |

Notes:

- `ratio = source chars / base85 chars`; > 1 means the payload is shorter than the source.
- Disjoint query parameter key/value split + dictionary compression optimizes query-heavy URLs with up to **1.400** compression ratio.

## Macro-Benchmarks: Tranco 1M and Top 10M Datasets

Tested across 14 diverse web archetypes (UUIDs, Git hashes, file extensions, REST APIs, e-commerce, tracking tags, media links, and queries):

```sh
# Tranco 1 Million benchmark
cargo run --release -p xtask -- bench-tranco tranco_L5QY4.csv 1000000

# Top 10 Million benchmark
cargo run --release -p xtask -- bench-tranco top10milliondomains.csv 10000000
```

| Metric | **Tranco 1 Million (1M URLs)** | **Top 10 Million (10M URLs)** |
|---|:---:|:---:|
| **Unique Domains** | 889,388 | **8,743,106** |
| **Total URLs Encoded** | 1,000,000 | **10,000,000** |
| **Encode Errors** | **0 (100% lossless)** | **0 (100% lossless)** |
| **Wall Clock Time** | **8.44 s** | **93.47 s (~1.5 min)** |
| **Throughput** | **118,475 URLs/s** | **106,983 URLs/s** |
| **Mean Latency** | **8.44 µs/url** | **9.35 µs/url** |
| **P50 / P90 / P99 Latency** | **7 / 15 / 18 µs** | **7 / 16 / 20 µs** |
| **Total Source Size** | 85.65 MB (85,651,178 chars) | **869.98 MB (869,977,173 chars)** |
| **Total Encoded Size** | 70.66 MB (70,658,513 chars) | **719.52 MB (719,521,068 chars)** |
| **Net Storage Saved** | **14.99 MB saved** | **150.46 MB saved** |
| **Overall Compression Ratio** | **1.212× (17.5% smaller)** | **1.209× (17.3% smaller)** |
| **Without Query Ratio** (Paths/Media) | **1.314× (23.9% smaller)** | **1.306× (23.5% smaller)** |
| **With Query Ratio** (Queries) | **1.165× (14.2% smaller)** | **1.163× (14.0% smaller)** |


## Per-URL Stateless Compression vs. General-Purpose Algorithms

When compressing individual URLs in isolation (stateless link shrinking for QR codes, BLE packets, SMS, or cache keys), general-purpose compression algorithms (DEFLATE, zlib, gzip) fail due to header overhead, lack of domain dictionaries, and LZ77 sliding window startup costs on short inputs:

### Empirical Benchmark (200,043 Real-World URLs from `corpus.txt`):

| Algorithm | Wire Transport | Ratio (`src/enc`) | Net Change | Status |
|---|:---:|:---:|:---:|:---|
| **`urlz` (Specialized Engine)** | **Base85** | **1.212×** | **17.5% smaller** | **Effective Compression** |
| **Raw DEFLATE (Level 9)** | Base85 | 0.852× | 17.4% larger | Negative Compression (Expansion) |
| **`zlib` (Header + Adler32)** | Base85 | 0.795× | 25.8% larger | Negative Compression (Expansion) |
| **`gzip` (RFC 1952 Header + CRC)** | Base85 | 0.697× | 43.5% larger | Negative Compression (Expansion) |

### Concrete Per-URL Comparison Examples:

| Original URL | Length | `urlz` | Raw DEFLATE + Base85 | `gzip` + Base85 |
|---|:---:|:---:|:---:|:---:|
| `https://example.com` | 19 chars | **11 chars** (42% smaller) | 27 chars (+42% larger) | 38 chars (+100% larger) |
| `https://example.com/index.html` | 30 chars | **14 chars** (53% smaller) | 40 chars (+33% larger) | 58 chars (+93% larger) |
| `https://github.com/rust-lang/rust` | 33 chars | **24 chars** (27% smaller) | 39 chars (+18% larger) | 54 chars (+64% larger) |
| `https://www.google.com/search?q=hello+world` | 43 chars | **28 chars** (35% smaller) | 57 chars (+33% larger) | 72 chars (+67% larger) |
| `https://example.com/search?q=rust+url+compression&page=2&sort=desc&filter=all` | 77 chars | **55 chars** (29% smaller) | 92 chars (+19% larger) | 108 chars (+40% larger) |

### Why General-Purpose Codecs Fail on Short Strings:
1. **Framing & Checksum Overhead**: `gzip` adds 18 bytes (header + CRC32 footer); `zlib` adds 6 bytes; dynamic DEFLATE blocks require tree headers. For a 30-character URL, headers alone exceed the payload.
2. **LZ77 Inefficiency**: LZ77 relies on matching back-references within a 32 KB sliding window. Individual URLs (30–80 bytes) have virtually zero internal substring repetition.
3. **Absence of URL Semantic Modeling**: General algorithms cannot exploit the fact that `https://`, `www.`, top-level domains (`.com`, `.org`), and query keys (`q=`, `page=`) can be mapped to compact 1-to-5-bit integer indices.

## Adversarial decode payloads

| Payload | Construction | Result |
|---|---|---|
| `large_65536` | 65536 bytes of `0xff` → base85 via `alphabet::to_base` (81800 chars) | no panic |
| `garbage_1024` | `"!"` × 1024 (invalid base85 payload) | no panic |
| `segments_64` | URL with 64 path segments, encoded (464 chars) | no panic |

A ~50KB repeated-path URL was also tried for the large payload, but the dictionary
compresses it to 19 chars, so the payload is synthesized directly from raw bytes.