# Changelog

All notable changes to this project will be documented in this file.

## [0.1.0] - 2026-08-23

### Added

- **`urlz` Core Library**:
  - Deterministic RFC 3986 URL parsing and semantic normalization.
  - Adaptive per-segment encoding across 8 radix alphabets (Base10, Base26, Base36, Base62, Base64url, Canonical Huffman, and Raw UTF-8 fallback).
  - High-performance MSB-first bitstream serializer with LEB128 varint encoding.
  - Base85 payload framing.
  - Defensive security boundaries against hostile inputs (64 KiB payload cap, 64 segments/region, 4,096 symbols/segment, zero-panic guarantees).
- **`urlz-cli` Binary Tool**:
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

[0.1.0]: https://github.com/cricsion/urlz/releases/tag/v0.1.0
