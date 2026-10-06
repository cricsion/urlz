# Changelog

All notable changes to this project will be documented in this file.

## [0.2.0] - 2026-10-03

- **Wire Format v2**:
  - Bumped canonical `WIRE_VERSION` to `2`.
  - Introduced compact counts (`write_compact_count` / `read_compact_count`), replacing 8-bit varints with 4-bit nibbles for counts $0 \le n \le 14$.
  - Eliminated per-segment `value_bit_length` from the wire by deriving bit lengths deterministically via `fixed_bit_len`.
  - Added explicit port encoding with 1-bit `port_flag` and 2-bit mode for common non-default ports (`8080`, `8443`, `3000`) or 16-bit raw integer fallback.
  - Added multi-label subdomain support with 1-bit `subdomain_flag` and 3-bit label count (2..=9 labels).
  - Allocated `alphabet_id = 7` (`PctBytes`) for 3× denser binary compression of pure percent-encoded (`%XX`) byte sequences.
  - Expanded static dictionaries: `COMMON_HOSTS` to 255 entries and `COMMON_PATH_TOKENS` to 128 entries.
- **Performance & Decoding Engine**:
  - Accelerated radix base conversions in `alphabet` via native `BigUint` radix operations (`to_radix_be` / `from_radix_be`).
  - Optimized `write_biguint_bits` bitstream packing with chunked 64-bit integer writes.
  - Single-pass O(N) alphabet classification in `analyze_segment` via static 256-byte bitmask lookup table.
  - Zero-allocation streaming decoder avoiding intermediate segment allocations and utilizing `Cow` for static dictionary tokens.
  - Single-pass host escape normalization and streaming `Display` formatter in `urlparse`.
  - Increased overall throughput to **448,000–450,000+ URLs/s** and reduced mean latency to **sub-2.25 µs**.
- **Benchmark Gains (v1 vs. v2)**:

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

## [0.1.1] - 2026-09-01

### Fixed

- Package CLI binary target (`src/main.rs`) properly in published crate to support `cargo install urlz`.

## [0.1.0] - 2026-08-31

### Added

- **`urlz` Core Library**:
  - Deterministic RFC 3986 URL parsing and semantic normalization.
  - Adaptive per-segment encoding across 8 radix alphabets (Base10, Base26, Base36, Base62, Base64url, Canonical Huffman, and Raw UTF-8 fallback).
  - High-performance MSB-first bitstream serializer with LEB128 varint encoding.
  - Base85 payload framing.
  - Defensive security boundaries against hostile inputs (64 KiB payload cap, 64 segments/region, 4,096 symbols/segment, zero-panic guarantees).
- **`urlz` Binary CLI**:
  - `encode`, `decode`, and `dict build` subcommands.
  - POSIX shell streaming integration with `xargs` for parallel batch processing.
- **Embedded Canonical Huffman Codebook**:
  - Embedded 256-byte canonical Huffman codebook trained over 1,000,000 real-world Tranco domains.
- **Tooling & Benchmarks**:
  - `xtask` workspace runner for large-scale benchmarks against Tranco 1M and Top 10M domain lists.
  - Criterion micro-benchmark suite (`encode_decode`).
- **Documentation Suite**:
  - Full wire format specification and system architecture deep dive ([`ARCHITECTURE.md`](ARCHITECTURE.md)).
  - Developer integration guide with Axum shortener, BLE beacon, and Rayon recipes ([`USAGE.md`](USAGE.md)).
  - Empirical compression benchmarks vs DEFLATE, zlib, and gzip ([`BENCH.md`](BENCH.md)).

[0.2.0]: https://github.com/cricsion/urlz/compare/v0.1.1...v0.2.0
[0.1.1]: https://github.com/cricsion/urlz/releases/tag/v0.1.1
[0.1.0]: https://github.com/cricsion/urlz/releases/tag/v0.1.0
