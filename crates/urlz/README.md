# urlz

[![Crates.io](https://img.shields.io/crates/v/urlz.svg)](https://crates.io/crates/urlz)
[![Docs.rs](https://docs.rs/urlz/badge.svg)](https://docs.rs/urlz)
[![License](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](LICENSE-MIT)

High-efficiency, deterministic URL compression engine in Rust.

`urlz` rewrites URLs into ultra-dense payloads by exploiting structural patterns: known hosts/TLDs become dictionary indices, boilerplate suffixes are elided, path/query segments are encoded into optimal radix alphabets, and canonical Huffman coding is applied when it saves bits on the wire.

---

## Quick Example

```rust
use urlz::{encode, decode, encode_to_bits, decode_bits};

fn main() -> Result<(), urlz::Error> {
    let url = "https://example.com/index.html";

    // 1. Text mode: Compact Base85 string (~2.14x compression)
    let payload = encode(url)?;
    assert_eq!(decode(&payload)?, url);

    // 2. Binary mode: Raw bitstream bytes for BLE, IoT, or packets
    let bits: Vec<u8> = encode_to_bits(url)?;
    assert_eq!(decode_bits(&bits)?, url);

    Ok(())
}
```

---

## Compression Sample

| Original URL | Length | `urlz` Base85 | Ratio |
|---|:---:|:---:|:---:|
| `https://example.com/index.html` | 30 chars | **14 chars** | **2.14×** |
| `https://www.google.com/search?q=hello+world` | 43 chars | **28 chars** | **1.54×** |
| `https://github.com/rust-lang/rust` | 33 chars | **24 chars** | **1.38×** |
| `https://example.com/search?q=rust&page=2&sort=desc` | 51 chars | **37 chars** | **1.38×** |

*Note: Encoded payloads are not guaranteed shorter on highly irregular or random strings; structured URLs benefit most.*

---

## Key Features

- ⚡ **High Throughput:** Encodes over **118,000 URLs/sec** with sub-9µs mean latency.
- 🛡️ **Hostile Input Resilience:** Strict memory boundaries (64 KiB payload cap, 64 segments/region). Rejects invalid padding, varint overflows, and bad indices as typed errors — **never panics**.
- 🗜️ **Adaptive Multi-Base & Huffman:** Automatically selects between 8 character sets (Base10, Base26, Base36, Base62, Base64url, Canonical Huffman, Raw UTF-8) to minimize wire size.
- 🌐 **Stateless & Offline:** Zero centralized databases, zero lookups, and no network dependencies.

---

## Documentation & Tooling

- **Repository & Architecture Deep Dive:** [github.com/cricsion/urlz](https://github.com/cricsion/urlz)
- **CLI Binary:** [`urlz-cli`](https://crates.io/crates/urlz-cli) (`cargo install urlz-cli`)
- **Specification:** [ARCHITECTURE.md](https://github.com/cricsion/urlz/blob/main/ARCHITECTURE.md)

## License

Dual-licensed under MIT OR Apache-2.0.
