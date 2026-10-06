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
| https://example.com/index.html | 30 | 11 | **2.727** |
| https://github.com/rust-lang/rust | 33 | 16 | **2.062** |
| https://example.com/%E4%B8%AD%E6%96%87/%E8%B7%AF%E5%BE%84 | 57 | 29 | **1.966** |
| https://example.com:8080/path | 29 | 16 | **1.812** |
| https://example.com | 19 | 11 | **1.727** |
| https://www.google.com/search?q=hello+world | 43 | 25 | **1.720** |
| https://example.com/search?q=rust+url+compression&page=2&sort=desc&filter=all | 77 | 50 | **1.540** |
| https://example.com/page#section-2 | 34 | 24 | **1.417** |
| https://example.com/a/b/c/d/e | 29 | 21 | **1.381** |
| https://example-site.com/x | 26 | 20 | **1.300** |

Notes:

- `ratio = source chars / base85 chars`; > 1 means the payload is shorter than the source.
- Specialized radix alphabets, token dictionaries, and canonical Huffman coding optimize diverse URL archetypes with up to **2.727×** compression ratio.

## Macro-Benchmarks: Tranco 1M and Top 10M Datasets

Tested on an Apple M3 MacBook Air across 14 diverse web archetypes (UUIDs, Git hashes, file extensions, REST APIs, e-commerce, tracking tags, media links, and queries):

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
| **Wall Clock Time** | **2.23 s** | **22.19 s** |
| **Throughput** | **447,974 URLs/s** | **450,728 URLs/s** |
| **Mean Latency** | **2.23 µs/url** | **2.22 µs/url** |
| **P50 / P90 / P99 Latency** | **1 / 2 / 3 µs** | **1 / 2 / 3 µs** |
| **Total Source Size** | 85.65 MB (85,651,178 chars) | **869.98 MB (869,977,173 chars)** |
| **Total Encoded Size** | 55.35 MB (55,348,350 chars) | **567.44 MB (567,443,631 chars)** |
| **Net Storage Saved** | **30.30 MB saved** | **302.53 MB saved** |
| **Overall Compression Ratio** | **1.547× (35.4% smaller)** | **1.533× (34.8% smaller)** |
| **Without Query Ratio** (Paths/Media) | **1.573× (36.4% smaller)** | **1.552× (35.6% smaller)** |
| **With Query Ratio** (Queries) | **1.534× (34.8% smaller)** | **1.524× (34.4% smaller)** |

## Wire Format Evolution: v1 vs. v2 Benchmark Comparison

The transition from wire format v1 to v2 eliminated per-segment `value_bit_length`, introduced compact count nibbles (0..14 as 4-bit nibbles), added 2-bit non-default port encoding, added `PctBytes` (contiguous `%XX` decoded to raw bytes), and streamlined 3-bit alphabet IDs.

### Key Benchmark Changes (v1 vs. v2)

| Metric | Wire Format v1 | Wire Format v2 (Current) | Improvement |
| :--- | :---: | :---: | :---: |
| **Real-World Corpus (200k URLs)** | 1.212× (17.5% smaller) | **1.558× (35.8% smaller)** | **+2.0× more savings** |
| **Tranco 1 Million Compression** | 1.212× (17.5% smaller) | **1.547× (35.4% smaller)** | **30.30 MB net saved** (was 14.99 MB) |
| **Top 10 Million Compression** | 1.209× (17.3% smaller) | **1.533× (34.8% smaller)** | **302.53 MB net saved** (was 150.46 MB) |
| **Tranco 1M Throughput** | 416,366 URLs/s | **447,974 URLs/s** | **+7.6% faster** |
| **Top 10M Throughput** | 413,269 URLs/s | **450,728 URLs/s** | **+9.1% faster** |
| **Mean Latency** | 2.40–2.42 µs/URL | **2.22–2.23 µs/URL** | **Sub-2.25 µs** |
| **P50 / P90 / P99 Latency** | 2 / 3 / 3 µs | **1 / 2 / 3 µs** | **P50 down to 1 µs** |
| **Adversarial 64-Segment Payload** | 464 chars | **333 chars** | **28% smaller wire payload** |

### Representative URL Compression (v1 vs. v2)

| Test URL | Source | v1 Encoded | v2 Encoded | v2 Ratio |
| :--- | :---: | :---: | :---: | :---: |
| `https://example.com/index.html` | 30 | 14 chars (2.14×) | **11 chars** | **2.727× (63% smaller)** |
| `https://github.com/rust-lang/rust` | 33 | 24 chars (1.38×) | **16 chars** (`GI_(e2`l6P2q,*{C`) | **2.062× (52% smaller)** |
| `https://example.com/%E4%B8%AD%E6%96%87/%E8%B7%AF%E5%BE%84` | 57 | 66 chars (0.86× expansion) | **29 chars** (via `PctBytes`) | **1.966× (49% smaller)** |
| `https://example.com:8080/path` | 29 | 31 chars (0.94× expansion) | **16 chars** (via port mode 0) | **1.812× (45% smaller)** |
| `https://example.com` | 19 | 12 chars (1.58×) | **11 chars** | **1.727× (42% smaller)** |
| `https://www.google.com/search?q=hello+world` | 43 | 27 chars (1.59×) | **25 chars** | **1.720× (42% smaller)** |
| `https://example.com/search?q=rust+url+compression...` | 77 | 55 chars (1.40×) | **50 chars** | **1.540× (35% smaller)** |
| `https://example.com/a/b/c/d/e` | 29 | 31 chars (0.94× expansion) | **21 chars** (no longer expands) | **1.381× (28% smaller)** |
| `https://example-site.com/x` | 26 | 24 chars (1.08×) | **20 chars** | **1.300× (23% smaller)** |

## Per-URL Stateless Compression vs. General-Purpose Algorithms

When compressing individual URLs in isolation (stateless link shrinking for QR codes, BLE packets, SMS, or cache keys), general-purpose compression algorithms (DEFLATE, zlib, gzip) fail due to header overhead, lack of domain dictionaries, and LZ77 sliding window startup costs on short inputs:

### Empirical Benchmark (200,043 Real-World URLs from `corpus.txt`):

| Algorithm | Wire Transport | Ratio (`src/enc`) | Net Change | Status |
|---|:---:|:---:|:---:|:---|
| **`urlz` (Specialized Engine)** | **Base85** | **1.558×** | **35.8% smaller** | **Effective Compression** |
| **Raw DEFLATE (Level 9)** | Base85 | 0.852× | 17.4% larger | Negative Compression (Expansion) |
| **`zlib` (Header + Adler32)** | Base85 | 0.795× | 25.8% larger | Negative Compression (Expansion) |
| **`gzip` (RFC 1952 Header + CRC)** | Base85 | 0.697× | 43.5% larger | Negative Compression (Expansion) |

### Concrete Per-URL Comparison Examples:

| Original URL | Length | `urlz` | Raw DEFLATE + Base85 | `gzip` + Base85 |
|---|:---:|:---:|:---:|:---:|
| `https://example.com` | 19 chars | **11 chars** (42% smaller) | 27 chars (+42% larger) | 38 chars (+100% larger) |
| `https://example.com/index.html` | 30 chars | **11 chars** (63% smaller) | 40 chars (+33% larger) | 58 chars (+93% larger) |
| `https://github.com/rust-lang/rust` | 33 chars | **16 chars** (52% smaller) | 39 chars (+18% larger) | 54 chars (+64% larger) |
| `https://www.google.com/search?q=hello+world` | 43 chars | **25 chars** (42% smaller) | 57 chars (+33% larger) | 72 chars (+67% larger) |
| `https://example.com/search?q=rust+url+compression&page=2&sort=desc&filter=all` | 77 chars | **50 chars** (35% smaller) | 92 chars (+19% larger) | 108 chars (+40% larger) |

### Why General-Purpose Codecs Fail on Short Strings:
1. **Framing & Checksum Overhead**: `gzip` adds 18 bytes (header + CRC32 footer); `zlib` adds 6 bytes; dynamic DEFLATE blocks require tree headers. For a 30-character URL, headers alone exceed the payload.
2. **LZ77 Inefficiency**: LZ77 relies on matching back-references within a 32 KB sliding window. Individual URLs (30–80 bytes) have virtually zero internal substring repetition.
3. **Absence of URL Semantic Modeling**: General algorithms cannot exploit the fact that `https://`, `www.`, top-level domains (`.com`, `.org`), and query keys (`q=`, `page=`) can be mapped to compact 1-to-5-bit integer indices.

## Adversarial decode payloads

| Payload | Construction | Result |
|---|---|---|
| `large_65536` | 65536 bytes of `0xff` → base85 via `alphabet::to_base` (81800 chars) | no panic |
| `garbage_1024` | `"!"` × 1024 (invalid base85 payload) | no panic |
| `segments_64` | URL with 64 path segments, encoded (333 chars) | no panic |

A ~50KB repeated-path URL was also tried for the large payload, but the dictionary
compresses it to 19 chars, so the payload is synthesized directly from raw bytes.