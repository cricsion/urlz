# Changelog

All notable changes to this project will be documented in this file.

## [Unreleased]

### Changed

- **Performance Optimizations**:
  - Accelerated radix base conversion in `alphabet::to_base` and `alphabet::from_base` using native `BigUint` radix operations (`to_radix_be` and `from_radix_be`).
  - Optimized `write_biguint_bits` bitstream packing with chunked 64-bit integer writes.
  - Increased overall throughput from ~118,000 URLs/s to **370,000+ URLs/s** and reduced mean latency from 8.44 µs to **2.69 µs**.

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

[0.1.1]: https://github.com/cricsion/urlz/releases/tag/v0.1.1
[0.1.0]: https://github.com/cricsion/urlz/releases/tag/v0.1.0
