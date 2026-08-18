# urlz-cli

[![Crates.io](https://img.shields.io/crates/v/urlz-cli.svg)](https://crates.io/crates/urlz-cli)
[![License](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](LICENSE-MIT)

Command-line interface and batch compression utility for the [`urlz`](https://crates.io/crates/urlz) URL compression codec.

---

## Installation

```sh
cargo install urlz-cli
```

---

## Usage

### 1. Encode & Decode Single URLs
```sh
# Encode a URL to a compact Base85 string
$ urlz encode "https://github.com/rust-lang/rust"
bB;p`O;%0@1j1m)T=Q3s9X!

# Decode back to the original URL
$ urlz decode "bB;p`O;%0@1j1m)T=Q3s9X!"
https://github.com/rust-lang/rust
```

### 2. High-Throughput Batch Processing via Shell Pipelines
```sh
# Compress 100,000 URLs from a log file in parallel
cat urls.txt | xargs -P 8 -n 500 -I {} urlz encode {} > compressed.txt
```

### 3. Build Custom Huffman Codebooks
```sh
# Generate an optimized 256-byte codebook from your access logs
urlz dict build access_urls.txt --out ./dictionaries
```

---

## Documentation & Library

- **Core Rust Library:** [`urlz`](https://crates.io/crates/urlz) (`cargo add urlz`)
- **System Architecture & Full Spec:** [github.com/cricsion/urlz](https://github.com/cricsion/urlz)

## License

Dual-licensed under MIT OR Apache-2.0.
