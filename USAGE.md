# urlz: User Guide & Developer Documentation

Comprehensive guide for using the `urlz` compression library and command-line tool.


## Table of Contents

1. [Installation](#1-installation)
   - [As a Rust Library](#as-a-rust-library)
   - [As a CLI Binary](#as-a-cli-binary)
2. [Quickstart](#2-quickstart)
   - [CLI Quickstart](#cli-quickstart)
   - [Library Quickstart](#library-quickstart)
3. [Rust Library Usage Guide](#3-rust-library-usage-guide)
   - [Basic Encoding & Decoding](#basic-encoding--decoding)
   - [Low-Level Bitstream Operations](#low-level-bitstream-operations)
   - [Custom Codebooks & Training](#custom-codebooks--training)
   - [Error Handling & Pattern Matching](#error-handling--pattern-matching)
4. [CLI Guide & Shell Recipes](#4-cli-guide--shell-recipes)
   - [Commands & Options](#commands--options)

5. [Architecture & Integration Recipes](#5-architecture--integration-recipes)
   - [Recipe 1: Stateless URL Shortener (Axum Web Service)](#recipe-1-stateless-url-shortener-axum-web-service)
   - [Recipe 2: IoT / BLE Advertisement Payload](#recipe-2-iot--ble-advertisement-payload)
   - [Recipe 3: High-Throughput Batch Processing with Rayon](#recipe-3-high-throughput-batch-processing-with-rayon)
6. [Security & Robustness Guarantees](#6-security--robustness-guarantees)

---

## 1. Installation

### As a Rust Library

Add `urlz` to your `Cargo.toml`:

```toml
[dependencies]
urlz = "0.1.0"
```

Or using `cargo add`:

```sh
cargo add urlz
```

### As a CLI Binary

Install the `urlz` command-line tool globally:

```sh
cargo install urlz
```

Or build from source in a cloned workspace:

```sh
cargo install --path crates/urlz
```

---

## 2. Quickstart

### CLI Quickstart

```sh
# Encode a URL to compact Base85
$ urlz encode "https://github.com/rust-lang/rust"
bB;p`O;%0@1j1m)T=Q3s9X!

# Decode a payload back to the original URL
$ urlz decode "bB;p`O;%0@1j1m)T=Q3s9X!"
https://github.com/rust-lang/rust
```

### Library Quickstart

```rust
use urlz::{decode, encode};

fn main() -> Result<(), urlz::Error> {
    let url = "https://github.com/rust-lang/rust";

    // Encode to a compact Base85 string (~1.38x compression)
    let payload = encode(url)?;
    println!("Payload: {payload}");

    // Decode back losslessly
    let restored = decode(&payload)?;
    assert_eq!(restored, url);

    Ok(())
}
```

---

## 3. Rust Library Usage Guide

### Basic Encoding & Decoding

The top-level `encode` and `decode` functions provide high-throughput string-to-string operations.

```rust
use urlz::{decode, encode};

fn main() -> Result<(), urlz::Error> {
    // 1. URLs with known hosts and TLDs compress heavily
    let url = "https://example.com/index.html";
    let compressed = encode(url)?;
    assert_eq!(decode(&compressed)?, url);

    // 2. Query-heavy URLs benefit from key/value dictionary indices
    let search_url = "https://www.google.com/search?q=rust+url+compression";
    let search_payload = encode(search_url)?;
    assert_eq!(decode(&search_payload)?, search_url);

    // 3. Port stripping & normalization
    let default_port = "https://example.com:443/api";
    let payload = encode(default_port)?;
    // Automatically normalizes standard ports (443 for HTTPS, 80 for HTTP)
    assert_eq!(decode(&payload)?, "https://example.com/api");

    Ok(())
}
```

### Low-Level Bitstream Operations

For embedded devices, network protocols, or binary serialization, use `encode_to_bits` and `decode_bits` to operate directly on raw big-endian byte buffers without Base85 string overhead:

```rust
use urlz::{decode_bits, encode_to_bits};

fn main() -> Result<(), urlz::Error> {
    let url = "https://example.com/api/v1/users";

    // Encode directly into raw bitstream bytes (Vec<u8>)
    let bitstream: Vec<u8> = encode_to_bits(url)?;
    println!("Bitstream byte length: {} bytes", bitstream.len());

    // Decode directly from raw bitstream slice
    let restored = decode_bits(&bitstream)?;
    assert_eq!(restored, url);

    Ok(())
}
```

### Custom Codebooks & Training

By default, `urlz` embeds an optimized 256-byte canonical Huffman codebook trained on millions of real-world URLs. You can also train domain-specific codebooks on your own URL datasets:

```rust
use urlz::huffman::{
    build_from_corpus, deserialize_codebook, parse_corpus, serialize_codebook, Codebook,
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let sample_corpus = "\
https://api.mycompany.internal/v1/projects/100/deployments
https://api.mycompany.internal/v1/projects/200/logs?tail=100
https://api.mycompany.internal/v1/users/admin/permissions\n";

    // 1. Strip scheme and host authority to extract resource shapes
    let training_text = parse_corpus(sample_corpus);

    // 2. Compute canonical Huffman codebook
    let codebook: Codebook = build_from_corpus(&training_text);

    // 3. Serialize to exactly 256 raw bytes
    let serialized: Vec<u8> = serialize_codebook(&codebook);
    assert_eq!(serialized.len(), 256);

    // 4. Deserialize on receiver
    let loaded: Codebook = deserialize_codebook(&serialized)?;
    assert_eq!(loaded, codebook);

    Ok(())
}
```

### Error Handling & Pattern Matching

`urlz` uses strongly-typed errors with descriptive messages and never panics on invalid input:

```rust
use urlz::{decode, encode, Error};

fn handle_url(input: &str) {
    match encode(input) {
        Ok(payload) => println!("Encoded: {payload}"),
        Err(Error::InvalidUrl { reason }) => {
            eprintln!("Failed to parse URL: {reason}");
        }
        Err(Error::UnsupportedCharacter(c)) => {
            eprintln!("Character '{c}' cannot be represented");
        }
        Err(e) => eprintln!("Encoding error: {e}"),
    }
}

fn handle_payload(payload: &str) {
    match decode(payload) {
        Ok(url) => println!("Decoded: {url}"),
        Err(Error::UnsupportedVersion(v)) => {
            eprintln!("Incompatible format version: {v}");
        }
        Err(Error::InvalidPayload { reason }) => {
            eprintln!("Corrupt or truncated payload: {reason}");
        }
        Err(e) => eprintln!("Decoding error: {e}"),
    }
}
```

---

## 4. CLI Guide & Shell Recipes

### Commands & Options

```sh
urlz <COMMAND>

Commands:
  encode  Encode a URL to a compact Base85 string
  decode  Decode a compressed Base85 string back to its URL
  dict    Manage and build compression dictionaries & codebooks
  help    Print this message or the help of the given subcommand(s)
```

#### 1. Encode

```sh
urlz encode https://example.com/index.html
```

#### 2. Decode

```sh
urlz decode "<payload>"
```

#### 3. Dictionary Build

```sh
# Build codebook from an access log or URL list file
urlz dict build access_urls.txt --out ./dictionaries

# Generates ./dictionaries/codebook.bin
```


---

## 5. Architecture & Integration Recipes

### Recipe 1: Stateless URL Shortener (Axum Web Service)

Unlike traditional database-backed shorteners (which require PostgreSQL/Redis lookups), `urlz` enables **completely stateless URL redirection**:

```rust
use axum::{
    extract::Path,
    http::StatusCode,
    response::{IntoResponse, Redirect},
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};

#[derive(Deserialize)]
struct ShrinkRequest {
    url: String,
}

#[derive(Serialize)]
struct ShrinkResponse {
    token: String,
    short_url: String,
}

async fn shrink_handler(Json(payload): Json<ShrinkRequest>) -> Result<Json<ShrinkResponse>, StatusCode> {
    let token = urlz::encode(&payload.url).map_err(|_| StatusCode::BAD_REQUEST)?;
    let short_url = format!("https://s.example.com/r/{}", token);
    Ok(Json(ShrinkResponse { token, short_url }))
}

async fn redirect_handler(Path(token): Path<String>) -> Result<Redirect, StatusCode> {
    let destination = urlz::decode(&token).map_err(|_| StatusCode::NOT_FOUND)?;
    Ok(Redirect::temporary(&destination))
}

pub fn app() -> Router {
    Router::new()
        .route("/api/shrink", post(shrink_handler))
        .route("/r/{token}", get(redirect_handler))
}
```

### Recipe 2: IoT / BLE Advertisement Payload

Broadcast dense URLs in BLE advertising packets (limited to 31 bytes):

```rust
use urlz::encode_to_bits;

fn make_ble_beacon_payload(target_url: &str) -> Result<[u8; 31], &'static str> {
    let bits = encode_to_bits(target_url).map_err(|_| "encode failed")?;
    if bits.len() > 28 {
        return Err("URL bitstream exceeds BLE advertisement capacity");
    }

    let mut packet = [0u8; 31];
    packet[0] = 0x02; // Flags length
    packet[1] = 0x01; // Flags data type
    packet[2] = 0x06; // General Discoverable Mode
    packet[3] = (bits.len() + 1) as u8; // Custom Service Data length
    packet[4] = 0x16; // Service Data 16-bit UUID
    packet[5..5 + bits.len()].copy_from_slice(&bits);

    Ok(packet)
}
```

### Recipe 3: High-Throughput Batch Processing with Rayon

Compress millions of URLs across all CPU cores:

```rust
use rayon::prelude::*;
use urlz::{encode, Result};

fn batch_compress_urls(urls: &[String]) -> Vec<Result<String>> {
    urls.par_iter()
        .map(|url| encode(url))
        .collect()
}
```

---

## 6. Security & Robustness Guarantees

The `urlz` decoder is engineered to treat all inputs as untrusted and potentially adversarial:

| Security Invariant | Guarantee & Implementation |
|---|---|
| **Max Payload Size** | Payloads over $65,536$ bytes are rejected immediately before integer parsing. |
| **Max Segments Limit** | Path, query, and fragment segment counts are strictly capped at $64$. |
| **Max Symbols Limit** | Individual segment symbol counts are strictly capped at $4,096$ symbols. |
| **Varint Overflow Guard** | Varints are capped at $10$ continuation groups ($u64$ limit); overflows return typed errors. |
| **Boundary Padding Check** | Any non-zero trailing bits in the byte alignment boundary are rejected. |
| **Zero Panic Invariant** | Decoder never panics on truncated, random, or out-of-range dictionary index payloads. |
