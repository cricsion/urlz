# urlz

[![Crates.io](https://img.shields.io/crates/v/urlz.svg)](https://crates.io/crates/urlz)
[![Docs.rs](https://docs.rs/urlz/badge.svg)](https://docs.rs/urlz)
[![License](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](LICENSE-MIT)

High-efficiency, deterministic URL compression engine and CLI in Rust.

`urlz` rewrites URLs into ultra-dense payloads by exploiting structural patterns: known hosts/TLDs become dictionary indices, boilerplate suffixes are elided, path/query segments are encoded into optimal radix alphabets, and canonical Huffman coding is applied when it saves bits on the wire.

---

## Installation

### As a Command-Line Tool
```sh
cargo install urlz                  # from crates.io
cargo install --path crates/urlz    # or from local checkout
```

### As a Rust Library Dependency
```toml
[dependencies]
urlz = "0.1.1"
```

---

## CLI Usage

```sh
# 1. Encode a URL to a compact Base85 string
$ urlz encode "https://github.com/rust-lang/rust"
#`H(KM&4L`p!vEE0}0TnPfwO

# 2. Decode a payload back to the original URL
$ urlz decode '#`H(KM&4L`p!vEE0}0TnPfwO'
https://github.com/rust-lang/rust

# 3. High-throughput parallel batch compression via shell pipelines
$ cat urls.txt | xargs -P 8 -n 500 -I {} urlz encode {} > compressed.txt

# 4. Build custom 256-byte Huffman codebooks from your own access logs
$ urlz dict build access_urls.txt --out ./dictionaries
```

---

## Library Usage

```rust
use urlz::{encode, decode, encode_to_bits, decode_bits};

fn main() -> Result<(), urlz::Error> {
    let url = "https://example.com/index.html";

    // 1. Text mode: Compact Base85 string
    let payload = encode(url)?;
    assert_eq!(decode(&payload)?, url);

    // 2. Binary mode: Raw bitstream bytes for BLE, IoT, or UDP packets
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

## Documentation & Recipes

- **Repository & Architecture Deep Dive:** [github.com/cricsion/urlz](https://github.com/cricsion/urlz)
- **Specification:** [ARCHITECTURE.md](https://github.com/cricsion/urlz/blob/main/ARCHITECTURE.md)
- **Practical User Guide & Recipes:** [`USAGE.md`](https://github.com/cricsion/urlz/blob/main/USAGE.md)
- **Benchmark Suite & Comparisons:** [`BENCH.md`](https://github.com/cricsion/urlz/blob/main/BENCH.md)

## License

Dual-licensed under MIT OR Apache-2.0.
