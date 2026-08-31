# urlz URL Compression Engine — Exhaustive System Architecture & Technical Deep Dive (v1.0.0)


**Target System:** `url_compressor` (`urlz` and `xtask`)  
**Specification Version:** v1 (Normative Wire Format & Architecture)  
**Repository Language:** Rust (Edition 2024, Workspace Resolver "3")  

---

## Table of Contents
1. [Module 1: High-Level System Context & Problem Domain](#module-1-high-level-system-context--problem-domain)
   - 1.1 Core Purpose & Value Proposition
   - 1.2 Domain Mental Model & Core Entities
   - 1.3 System Boundaries & Operational Environment
2. [Module 2: Tech Stack & Dependency Selection Analysis](#module-2-tech-stack--dependency-selection-analysis)
3. [Module 3: Structural Architecture & Directory Breakdown](#module-3-structural-architecture--directory-breakdown)
   - 3.1 Repository Anatomy
   - 3.2 Component & Service Topology
   - 3.3 Dependency Flow & Clean Architecture Boundaries
4. [Module 4: Module-by-Module Inner Workings](#module-4-module-by-module-inner-workings)
   - 4.1 `alphabet.rs` — Compression Alphabets, Registry, $O(1)$ Inverses & Radix Math
   - 4.2 `bitstream.rs` — Bit-Level Stream Reader, Writer & Varint Codec
   - 4.3 `dict.rs` — Static TLD, Host, Path & Query Codebooks
   - 4.4 `urlparse.rs` — RFC 3986 URL Parser, Semantic Slicer & Normalizer
   - 4.5 `segment.rs` — Segment Analyzer & Multi-Base Integer Packing
   - 4.6 `huffman.rs` — Canonical Huffman Codebook & Dynamic Bitstream Engine
   - 4.7 `encode.rs` — Bitstream Serializer & Base85 Wire Formatter
   - 4.8 `decode.rs` — Bitstream Deserializer & Resilient URL Synthesizer
   - 4.9 `error.rs` — Strongly-Typed Error Architecture
   - 4.10 `crates/urlz/src/main.rs` — CLI Dispatcher & Ergonomics
5. [Module 5: End-to-End Execution Paths & Sequence Traces](#module-5-end-to-end-execution-paths--sequence-traces)
   - 5.1 Trace 1: Standard URL Encoding to Base85 Payload (`encode::encode`)
   - 5.2 Trace 2: Decoding & Validation (`decode::decode`)
   - 5.3 Trace 3: Codebook Training & Building from URL Corpus (`urlz dict build`)
   - 5.4 Trace 4: Adversarial / Corrupt Payload Rejection & Recovery
6. [Module 6: Design Patterns & Performance](#module-6-design-patterns--performance)
   - 6.1 Design Patterns Implemented
   - 6.2 Performance & Benchmarks

---

# Module 1: High-Level System Context & Problem Domain

## 1.1 Core Purpose & Value Proposition
The **urlz URL Compression System** is an ultra-dense, deterministic, lossless URL compressor written in Rust.

### The Exact Problem It Solves
Standard URLs are structurally redundant and verbosely formatted:
1. **Scheme & Subdomain Boilerplate:** Repeated `https://`, `http://`, and `www.` prefixes consume bytes without adding informational entropy.
2. **Domain & TLD Frequency:** Common TLDs (`.com`, `.org`, `.net`) and popular domains (`google`, `github`, `youtube`) appear repeatedly across billions of links.
3. **Structured Segment Redundancy:** Path elements, query keys (`utm_source`, `page`, `q`), and common values can be represented with specialized, dense character sets rather than full 8-bit ASCII.

### Who It Is For
- **Embedded & IoT Systems:** Constrained devices transmitting URLs over low-bandwidth physical or radio links (e.g., BLE advertisements, NFC, SMS, e-ink displays).
- **High-Density Storage & Messaging:** Systems storing billions of URLs where 15–25% reduction yields hundreds of gigabytes or terabytes of space savings.
- **Stateless URL Shrinking:** Systems needing deterministic, client-side URL compression without centralized database roundtrips or DNS lookups (unlike traditional URL shorteners like `bit.ly`).

### Functional & Non-Functional Requirements
- **Functional Requirements:**
  - Lossless, deterministic bidirectional conversion: $\text{URL} \longleftrightarrow \text{Bitstream} \longleftrightarrow \text{Base85 Payload}$.
  - Structural URL semantic parsing: Normalizing scheme (`http`/`https`), `www.` prefix, host/TLD extraction, trailing index file patterns (`index.html`, `index.php`), path hierarchies, ordered query pairs (`key=value`), and fragment segments (`#seg1/seg2`).
  - Adaptive per-segment alphabet selection across 8 distinct encodings (Base10, Base26-lower, Base36, Base62, Base64url, Huffman over Base85, Raw byte fallback).
  - Offline canonical Huffman codebook training over corpus files.
- **Non-Functional Requirements:**
  - **Memory Safety & Zero-Panic Guarantee:** Strict validation on all variable-length inputs (varints, bit lengths, symbol counts); malformed or adversarial payloads must return typed errors (`Error`) without crashing or panicking.
  - **Zero-Allocation Hot Paths:** Streaming bit-level encoding/decoding without intermediate heap bloat.
  - **High Throughput:** 140,000+ URLs/sec single-core encoding performance with sub-7µs mean latency.
  - **Bounded Resource Limits:** Hard caps preventing decompression bombs (Payload $\le 65,536$ bytes, Segments/Region $\le 64$, Symbols/Segment $\le 4,096$).

---

## 1.2 Domain Mental Model & Core Entities

```
                    ┌─────────────────────────────────────────────────────────┐
                    │                       Source URL                        │
                    │   https://www.google.com/search?q=rust#section-1        │
                    └────────────────────────────┬────────────────────────────┘
                                                 │ [urlparse::parse_url]
                                                 ▼
                    ┌─────────────────────────────────────────────────────────┐
                    │                   Parsed URL Struct                     │
                    │  - Scheme: HTTPS (true)                                 │
                    │  - WWW: true                                            │
                    │  - Host: "google" (matches COMMON_HOSTS[0])             │
                    │  - TLD: "com" (matches KNOWN_TLDS[0])                   │
                    │  - Index Suffix: None                                   │
                    │  - Path: ["search"]                                     │
                    │  - Query: [("q", Some("rust"))]                         │
                    │  - Fragment: ["section-1"]                              │
                    └────────────────────────────┬────────────────────────────┘
                                                 │ [segment::analyze_segment]
                                                 │ [huffman::HuffmanEncoder]
                                                 ▼
                    ┌─────────────────────────────────────────────────────────┐
                    │               Raw Big-Endian Bitstream                  │
                    │  [Header: 12b][Host: 2b+8b][TLD: 1b+5b][Resource: 3b...]│
                    └────────────────────────────┬────────────────────────────┘
                                                 │ [alphabet::to_base]
                                                 ▼
                                ┌─────────────────────────────────┐
                                │        Base85 Transport         │
                                │   `alphabet::BASE85_ALPHABET`   │
                                │   Density: ~6.41 bits/char      │
                                │   Standard wire string          │
                                └─────────────────────────────────┘
```

### Core Entities & Definitions
- **Normalized URL:** Canonicalized URL with lowercased scheme/host, stripped default ports (`:80`, `:443`), uppercase percent-escapes (`%2F`), and explicit empty segment preservation.
- **Dictionary Set ID (`DICT_SET_ID` = 1):** 4-bit header value binding the payload to static host/TLD codebooks.
- **Varint (Variable-Length Quantity):** 7-bit continuation LEB128-style serialization packed MSB-first into the bitstream (up to 10 groups covering a full `u64`).
- **Alphabet Registry:** An 8-slot lookup table mapping 4-bit IDs (`0..=7`) to numeric radixes and character subsets.
- **Segment:** A discrete URL component (path step, query key/value, or fragment token) encoded using its minimal sufficient alphabet or canonical Huffman codebook.
- **Canonical Huffman Codebook:** Prefix-free, deterministic binary codebook generated from symbol frequencies, serialized as 256 contiguous length bytes.
- **Base85 Wire Format:** 85 printable ASCII characters (excluding 9 problematic characters: `" ' \ % + / = < >`) mapping large big-endian integers to compact text.

---

## 1.3 System Boundaries & Operational Environment

```
      +-------------------------------------------------------------------------------+
      |                               OPERATING SYSTEM                                |
      |                                                                               |
      |  [ CLI Invocation / Shell ]                  [ Local Filesystem ]             |
      |          │                                             ▲                      |
      |          │ (argv / stdin)                              │ (read corpus file /  |
      |          ▼                                             │  write codebook.bin) |
      |  +-----------------------------------------------------+-------------------+  |
      |  | urlz CLI Binary (crates/urlz/src/main.rs)                           |  |
      |  |   - clap argument parser                                                |  |
      |  |   - anyhow error context                                                |  |
      |  +-------------------------+---------------------------+-------------------+  |
      |                            │                           │                      |
      |                            ▼                           │                      |
      |  +-----------------------------------------------------+-------------------+  |
      |  | crates/urlz (Core Compression Engine)                                   |  |
      |  |                                                                         |  |
      |  |  +------------------+  +-------------------+  +----------------------+  |  |
      |  |  |   urlparse.rs    |  |    segment.rs     |  |      huffman.rs      |  |  |
      |  |  | RFC 3986 Parser  |  | Alphabet Selector |  | Canonical Huffman    |  |  |
      |  |  +--------┬---------+  +---------┬---------+  +----------┬-----------+  |  |
      |  |           │                      │                       │              |  |
      |  |           ▼                      ▼                       ▼              |  |
      |  |  +------------------+  +-------------------+  +----------------------+  |  |
      |  |  |     dict.rs      |  |   bitstream.rs    |  |     alphabet.rs      |  |  |
      |  |  | Static Codebooks |  | MSB-first Reader/ |  | BigUint Base-N &     |  |  |
      |  |  | (TLDs/Hosts/Keys)|  | Writer            |  | O(1) Inverse Tables  |  |  |
      |  |  +--------┬---------+  +---------┬---------+  +----------┬-----------+  |  |
      |  |           │                      │                       │              |  |
      |  |           +──────────────────────┼───────────────────────+              |  |
      |  |                                  ▼                                      |  |
      |  |                        +-------------------+                            |  |
      |  |                        | encode.rs / decode|                            |  |
      |  |                        | Bitstream Assembly|                            |  |
      |  |                        +-------------------+                            |  |
      |  +-------------------------------------------------------------------------+  |
      +-------------------------------------------------------------------------------+
```

---

# Module 2: Tech Stack & Dependency Selection Analysis

| Technology / Crate | Version | Role in Stack | Selection Rationale | Alternatives Considered | Trade-offs & Operational Characteristics |
|---|---|---|---|---|---|
| **Rust** | `2024 Edition` | Core Language | Zero-cost abstractions, deterministic memory control without garbage collection pauses, guaranteed memory safety, bitwise manipulation primitives. | C99/C++20, Go, Zig | Maximum developer velocity for low-level bit operations; strict compiler verification; clean cross-compilation. |
| **`bitstream-io`** | `2.x` | Bitstream Serializer / Deserializer | Provides verified Big-Endian (MSB-first) bit-level reading/writing over arbitrary sinks (`Vec<u8>`, byte slices). | Hand-rolled bit buffers, `bitflags`, `nom` | Zero-allocation wrapping over byte slices. Minimizes custom bit-shift bugs; small wrapper overhead abstracted inside `bitstream.rs`. |
| **`num-bigint` & `num-traits`** | `0.4` / `0.2` | Arbitrary-Precision Radix Conversion | URLs produce variable-length bitstreams. `BigUint` enables lossless conversion to/from base-10, 26, 36, 62, 64, and 85. | `u128` (fixed limit), `rug` (GMP wrapper, requires C FFI), stack `U512` | Pure Rust, portable across embedded/WASM targets; clean arbitrary-precision radix division. |
| **`thiserror`** | `2.x` | Domain Error Definition (`urlz`) | Generates standard, zero-overhead `std::error::Error` implementations with clean enum variants for library consumers. | `quick-error`, manual `Display`/`Error` impls | Pure compile-time macro expansion; zero runtime overhead. |
| **`anyhow`** | `1.x` | Application Error Handling (CLI) | Provides idiomatic context attachment (`.with_context()`) for CLI file operations and exit codes. | Custom CLI error enums, `eyre` | Fast CLI error propagation; not used in library core to preserve typed error guarantees. |
| **`clap`** | `4.x` (features: `["derive"]`) | Command-Line Interface Parser | Declarative CLI interface with auto-generated help, subcommand dispatch, and type validation. | `lexopt`, `argh`, `pico-args` | Ergonomic CLI interface with subcommands and flags. |
| **`criterion`** | `0.5` | Micro-benchmarking Framework | Statistical benchmarking for encode/decode throughput, compression ratios, and adversarial payload safety. | `divan`, `bencher` | Robust statistical analysis (regression detection, variance filtering); generates output for `BENCH.md`. |
| **`proptest`** | `1.x` | Property-Based Testing | Exhaustive property testing of base conversion roundtrips, URL parsing invariants, and fuzzing. | `quickcheck`, `cargo-fuzz` | Seamless integration with `cargo test`; generates randomized inputs to find edge-case failures. |

---

# Module 3: Structural Architecture & Directory Breakdown

## 3.1 Repository Anatomy

```
url_compressor/
├── Cargo.toml                                 # Workspace manifest (Edition 2024, resolver "3")
├── Cargo.lock                                 # Exact pinned dependency lockfile
├── USAGE.md                                   # Practical User Guide, CLI recipes, and integration code
├── BENCH.md                                   # Benchmark criteria, methodology, and latency tables
├── ARCHITECTURE.md                            # Exhaustive wire spec & architecture deep dive (this document)
├── CHANGELOG.md                               # Versioned release changelog
├── crates/
│   ├── xtask/                                 # Workspace Tooling & Dataset Benchmarking
│   │   ├── Cargo.toml
│   │   └── src/
│   │       └── main.rs                        # Tranco benchmark runner and corpus generator
│   └── urlz/                                  # Core Library & CLI Crate
│       ├── Cargo.toml                         # Library & Binary manifest
│       ├── assets/
│       │   ├── codebook.bin                   # Shipped 256-byte canonical Huffman codebook
│       │   └── corpus.txt                     # Training URL corpus
│       ├── benches/
│       │   └── encode_decode.rs               # Criterion benchmark suites (throughput & ratios)
│       ├── tests/
│       │   ├── cli.rs                         # End-to-end CLI integration tests
│       │   └── roundtrip.rs                   # Full URL round-trip integration tests
│       └── src/
│           ├── lib.rs                         # Crate root, module exports & public re-exports
│           ├── main.rs                        # CLI entry point, argument parsing & subcommands
│           ├── error.rs                       # Strongly-typed error enum definitions
│           ├── alphabet.rs                    # Base85 charset, O(1) inverse tables, Base-N radix math
│           ├── bitstream.rs                   # WriteBitStream, ReadBitStream, & Varint codec
│           ├── dict.rs                        # Static TLDs (32), Hosts (40), Tokens (64), Query Keys (64)
│           ├── urlparse.rs                    # RFC 3986 URL parsing & semantic decomposition
│           ├── segment.rs                     # Alphabet selection & segment integer packing
│           ├── huffman.rs                     # Canonical Huffman coding & codebook builder
│           ├── encode.rs                      # Primary encoder (URL -> Bits -> Base85)
│           └── decode.rs                      # Primary decoder (Payload -> Bits -> URL)
```

---

## 3.2 Component & Service Topology

```
                                      +------------------------------------+
                                      |         urlz (CLI Binary)          |
                                      |  (CLI Command Dispatcher/Runner)   |
                                      +-----------------┬------------------+
                                                        │
                    ┌───────────────────────────────────┼───────────────────────────────────┐
                    │ Command::Encode                   │ Command::Decode                   │ Command::Dict
                    ▼                                   ▼                                   ▼
         +---------------------+             +---------------------+             +---------------------+
         |  encode::encode()   |             |  decode::decode()   |             |huffman::build_from_ |
         |                     |             |                     |             |      corpus()       |
         +----------┬----------+             +----------┬----------+             +----------┬----------+
                    │                                   │                                   │
                    └───────────────────────────────────┼───────────────────────────────────┘
                                                        │
                                                        ▼
+-------------------------------------------------------------------------------------------------------+
|                                                urlz                                                   |
|                                                                                                       |
|  +--------------------+      +--------------------+      +-----------------------------------------+  |
|  |    urlparse.rs     | ---> |     segment.rs     | ---> |               huffman.rs                |  |
|  |  Semantic Slicing  |      | Alphabet Selection |      |       Canonical Huffman Codebook        |  |
|  +--------------------+      +--------------------+      +-----------------------------------------+  |
|            │                          │                                       │                       |
|            ▼                          ▼                                       ▼                       |
|  +--------------------+      +--------------------+      +-----------------------------------------+  |
|  |      dict.rs       |      |    bitstream.rs    |      |               alphabet.rs               |  |
|  | Static Codebooks   |      | WriteBitStream /   |      |        Radix Math & Inverse Tables      |  |
|  | (TLDs/Hosts/Tokens)|      | ReadBitStream      |      |           (Base85 / Multi-Radix)        |  |
|  +--------------------+      +--------------------+      +-----------------------------------------+  |
|            │                          │                                       │                       |
|            └──────────────────────────┼───────────────────────────────────────┘                       |
|                                       ▼                                                               |
|                        +-----------------------------+                                                |
|                        |          encode.rs          |                                                |
|                        | Bitstream Assembly (Header, |                                                |
|                        | Host, TLD, Resources)       |                                                |
|                        +-----------------------------+                                                |
+-------------------------------------------------------------------------------------------------------+
```

---

## 3.3 Dependency Flow & Clean Architecture Boundaries

```
[ Domain Entities / Rules ]
  └─► alphabet.rs  (ALPHABETS, BASE85_ALPHABET, ALPHABET_INV, BASE85_INV)
  └─► dict.rs      (KNOWN_TLDS, COMMON_HOSTS, COMMON_PATH_TOKENS, COMMON_QUERY_KEYS, COMMON_QUERY_VALUES)
  └─► error.rs     (Error)

[ Core Primitives / Transformations ]
  └─► bitstream.rs (WriteBitStream, ReadBitStream)
  └─► urlparse.rs  (parse_url, ParsedUrl, IndexSuffix)
  └─► segment.rs   (analyze_segment, SegmentEncoding, value_to_biguint)
  └─► huffman.rs   (Codebook, HuffmanEncoder, HuffmanDecoder, build_codebook)

[ Application Services / Orchestration ]
  └─► encode.rs    (encode, encode_to_bits)
  └─► decode.rs    (decode, decode_bits)

[ Delivery Mechanism / Presentation ]
  └─► main.rs      (Cli, Command, DictCommand)
```

---

# Module 4: Module-by-Module Inner Workings

---

## 4.1 `alphabet.rs` — Compression Alphabets, Registry, $O(1)$ Inverses & Radix Math

### Purpose & Responsibilities
The `alphabet.rs` module is the foundational mathematical layer of urlz. It owns:
1. The definition of the primary wire character set: `BASE85_ALPHABET`.
2. The 8-entry canonical alphabet registry (`ALPHABETS`) defining 4-bit IDs (`0..=7`).
3. Compile-time precomputed 256-byte inverse lookup tables (`ALPHABET_INV` and `BASE85_INV`) for $O(1)$ character validation without allocations.
4. Arbitrary-precision base-conversion functions transforming between `num_bigint::BigUint` numeric values and positional symbol strings in any radix $N \le 85$.

### Key Types & Data Structures

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AlphabetInfo {
    pub id: u8,                // 4-bit identifier (0..=7)
    pub name: &'static str,    // Descriptive name (e.g. "base10", "base62")
    pub chars: &'static [u8],  // Byte table for base-N conversion (empty for IDs 5, 6, 7)
}
```

```
+--------------------------------------------------------------------------------------------------+
|                                      urlz ALPHABET REGISTRY                                      |
+----+---------------+-------+----------------------------------------------------+----------------+
| ID | Identifier    | Radix | Character Set Slices                               | Usage Domain   |
+----+---------------+-------+----------------------------------------------------+----------------+
| 0  | base10        | 10    | b"0123456789"                                      | Numeric paths  |
| 1  | base26-lower  | 26    | b"abcdefghijklmnopqrstuvwxyz"                      | Low-case hosts |
| 2  | base36        | 36    | b"0123456789abcdefghijklmnopqrstuvwxyz"             | Alphanum-lower |
| 3  | base62        | 62    | b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdef..."   | Mixed-case ID  |
| 4  | base64url     | 64    | b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdef...0123456789-_" | URL-safe base64|
| 5  | huffman-mode  | (85)  | &[] (Uses BASE85_ALPHABET via canonical Huffman)   | General ASCII  |
| 6  | raw-fallback  | (256) | &[] (Raw UTF-8 / binary 0x00..=0xFF)               | Foreign UTF-8  |
| 7  | reserved      | 0     | &[] (Future expansion; decoder rejects)            | Reserved       |
+----+---------------+-------+----------------------------------------------------+----------------+
```

### Exact Character Sets
- **`BASE85_ALPHABET` (85 bytes):**
  `!#$&()*,-.0123456789:;?@ABCDEFGHIJKLMNOPQRSTUVWXYZ[]^_`abcdefghijklmnopqrstuvwxyz{|}~`
  *(All printable ASCII `0x21`–`0x7E` excluding 9 characters: `"` (`0x22`), `'` (`0x27`), `\` (`0x5C`), `%` (`0x25`), `+` (`0x2B`), `/` (`0x2F`), `=` (`0x3D`), `<` (`0x3C`), and `>` (`0x3E`)).*

### Inverse Lookup Tables ($O(1)$ Optimization)
- **`BASE85_INV: [i8; 256]`**: Maps any byte `b` to its index in `BASE85_ALPHABET` (or `-1` if invalid).
- **`ALPHABET_INV: [[i8; 256]; 5]`**: Compile-time constant 2D table mapping `[alphabet_id][byte]` to its positional index, replacing linear scans with a single array read.

---

## 4.2 `bitstream.rs` — Bit-Level Stream Reader, Writer & Varint Codec

### Purpose & Responsibilities
The `bitstream.rs` module provides bit-granularity serialization and deserialization over byte buffers in **Big-Endian (MSB-first)** bit order. It encapsulates `bitstream-io` and implements the variable-length unsigned integer (**Varint**) protocol.

### Key Types & Data Structures

```rust
pub struct WriteBitStream {
    writer: BitWriter<Vec<u8>, BigEndian>,
    bit_len: usize,
}

pub struct ReadBitStream<'a> {
    reader: BitReader<&'a [u8], BigEndian>,
    bytes: &'a [u8],
    bit_pos: usize,
}
```

### Bit Layout & Varint Architecture
urlz varints encode a `u64` value into 8-bit groups containing 7 data bits and 1 continuation bit, written in least-significant group order (little-endian groups), but with each individual byte written **MSB-first**:

```
Varint Byte Structure:
 ┌───────────────────┬────────────────────────────────────────────────────────┐
 │ Bit 7 (MSB)       │ Bits 6..0                                              │
 │ Continuation Bit  │ 7 Data Bits (LSB group first)                          │
 └───────────────────┴────────────────────────────────────────────────────────┘
  1 = More bytes follow
  0 = Final byte of varint
```

```
Example: Encoding Value 300 (0b0000_0001_0010_1100)
 1. Group 0 (LSB 7 bits): 300 & 0x7F = 44 (0b010_1100). Cont = 1 -> Byte = 0b1010_1100 (0xAC)
 2. Group 1 (Next 7 bits): (300 >> 7) & 0x7F = 2 (0b000_0010). Cont = 0 -> Byte = 0b0000_0010 (0x02)
 Resulting Wire Stream: [0xAC, 0x02] (16 bits)
```

### Varint Codec Reference Logic

```rust
// WRITE VARINT (value: u64)
let mut v = value;
loop {
    let group = (v & 0x7F) as u8;
    v >>= 7;
    let cont = if v > 0 { 0x80 } else { 0x00 };
    write_bits((cont | group) as u64, 8); // MSB-first: continuation bit first
    if v == 0 { break; }
}

// READ VARINT -> Result<u64, Error>
let mut result = 0u64;
let mut shift = 0;
for _ in 0..10 {
    let byte = read_bits(8)?;
    let data = byte & 0x7F;
    let cont = (byte & 0x80) != 0;
    result |= data << shift;
    shift += 7;
    if !cont { return Ok(result); }
}
return Err(Error::InvalidPayload("varint overflow: >10 groups"));
```

---

## 4.3 `dict.rs` — Static TLD, Host, Path & Query Codebooks

### Purpose & Responsibilities
The `dict.rs` module manages static, immutable dictionary tables for Top-Level Domains (TLDs), frequent hostnames, common path tokens, and query keys/values. The ordering and indices are **normative and permanent** across all implementations.

### Key Dictionaries & Layout

| Dictionary | Entries | Index Range | Purpose |
|------------|:-------:|:-----------:|---------|
| `KNOWN_TLDS` | 32 | 0-31 | Common TLDs (`com`, `org`, `net`, etc. Sentinel 31 = bare host/IP) |
| `COMMON_HOSTS` | 40 | 0-39 | Popular domains (`google`, `github`, `youtube`, etc. 255 = escape) |
| `COMMON_PATH_TOKENS` | 64 | 0-63 | Common path segments (`api`, `v1`, `users`, `posts`, `app`, etc.) |
| `COMMON_QUERY_KEYS` | 64 | 0-63 | Common query keys (`utm_source`, `q`, `page`, `id`, `lang`, etc.) |
| `COMMON_QUERY_VALUES` | 32 | 0-31 | Common query values (`true`, `false`, `1`, `0`, `json`, etc.) |

### Protocol Constants & Sentinels

```rust
pub const DICT_SET_ID: u8 = 1;      // Header protocol identifier
pub const TLD_ESCAPE: u8 = 31;      // Sentinel: Bare host / IP / No TLD
pub const HOST_ESCAPE: u8 = 255;    // Sentinel: Escape to literal host mode
```

The canonical, immutable string arrays are defined in [`crates/urlz/src/dict.rs`](crates/urlz/src/dict.rs). Index ordering is normative across all v1 implementations.

---

## 4.4 `urlparse.rs` — RFC 3986 URL Parser, Semantic Slicer & Normalizer

### Purpose & Responsibilities
The `urlparse.rs` module parses arbitrary raw URL strings into structured components without data loss. It normalizes case, cleans up default ports, preserves structural empty segments, and extracts compression hints.

### Key Types & Data Structures

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedUrl {
    pub https: bool,                                   // true: https://, false: http://
    pub www: bool,                                     // true: host had "www." prefix
    pub host: String,                                 // Hostname without TLD or "www."
    pub tld: String,                                  // Extracted TLD (empty for localhost/IP)
    pub index_suffix: IndexSuffix,                    // Trailing index file hint
    pub path_segments: Vec<String>,                   // Split path segments
    pub query_segments: Vec<(String, Option<String>)>,// Key-value query pairs
    pub fragment_segments: Vec<String>,               // Fragment tokens split on '/'
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IndexSuffix {
    None,
    IndexHtml,         // Exactly "index.html"
    IndexPhp,          // Exactly "index.php"
    Other(String),     // Other "index.*" files (e.g. "index.aspx", "INDEX.HTML")
}
```

---

## 4.5 `segment.rs` — Segment Analyzer & Multi-Base Integer Packing

### Purpose & Responsibilities
The `segment.rs` module inspects string tokens from the path, query, fragment, or host, and selects the smallest sufficient alphabet from the registry to minimize bit consumption.

```
                      ┌──────────────────────────────┐
                      │    Input Segment String s    │
                      └──────────────┬───────────────┘
                                     │
                 ┌───────────────────┴───────────────────┐
                 │ Is s empty ("")?                      │
                 └─┬───────────────────────────────────┬─┘
              Yes  │                                   │ No
                   ▼                                   ▼
        ┌──────────────────────┐             ┌───────────────────┐
        │  alphabet_id = 0     │             │  Check Base10     │── All digits? ──► alphabet_id = 0
        │  symbol_count = 0    │             └─────────┬─────────┘
        │  value = []          │                       │ No
        └──────────────────────┘                       ▼
                                             ┌───────────────────┐
                                             │ Check Base26-low  │── All a-z? ─────► alphabet_id = 1
                                             └─────────┬─────────┘
                                                       │ No
                                                       ▼
                                             ┌───────────────────┐
                                             │   Check Base36    │── All 0-9,a-z? ─► alphabet_id = 2
                                             └─────────┬─────────┘
                                                       │ No
                                                       ▼
                                             ┌───────────────────┐
                                             │   Check Base62    │── All alphanum? ─► alphabet_id = 3
                                             └─────────┬─────────┘
                                                       │ No
                                                       ▼
                                             ┌───────────────────┐
                                             │  Check Base64url  │── Contains _ - ? ─► alphabet_id = 4
                                             └─────────┬─────────┘
                                                       │ No
                                                       ▼
                                             ┌───────────────────┐
                                             │   Raw Fallback    │── Arbitrary UTF-8 ─► alphabet_id = 6
                                             │ (Base 256 bytes)  │
                                             └───────────────────┘
```

---

## 4.6 `huffman.rs` — Canonical Huffman Codebook & Dynamic Bitstream Engine

### Purpose & Responsibilities
The `huffman.rs` module provides variable-length statistical prefix coding over the 85 symbols of `BASE85_ALPHABET`. It includes codebook synthesis via a min-heap arena, Kraft inequality validation, canonical code assignment, and bitstream encoding/decoding.

### Key Types & Data Structures

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Codebook(pub [u8; 256]);

pub struct HuffmanEncoder {
    codes: [u64; 256],      // Canonical binary prefix codes
    lengths: [u8; 256],     // Bit length of each symbol's code
}

pub struct HuffmanDecoder {
    sorted: Vec<u8>,        // Symbols sorted by (length, symbol)
    first_code: [u64; 65],  // Smallest canonical code for each bit length 1..=64
    first_index: [usize; 65],// Offset into `sorted` for each bit length
    count: [usize; 65],     // Number of symbols for each bit length
}
```

---

## 4.7 `encode.rs` — Bitstream Serializer & Base85 Wire Formatter

### Payload Bit Layout Specification

```
+--------------------------------------------------------------------------------------------------+
|                                    urlz v1 BITSTREAM LAYOUT                                    |
+--------------------------------------------------------------------------------------------------+
| Field Name               | Width (Bits) | Description / Value Encoding                           |
+--------------------------+--------------+--------------------------------------------------------+
| 1. HEADER (12 Bits)                                                                              |
|   version                | 4            | Literal 1 (v1)                                         |
|   dict_set_id            | 4            | Literal 1 (DICT_SET_ID)                                |
|   https_flag             | 1            | 1 = HTTPS, 0 = HTTP                                    |
|   www_flag               | 1            | 1 = "www." present, 0 = absent                         |
|   index_suffix           | 2            | 0 = None, 1 = index.html, 2 = index.php, 3 = Other     |
+--------------------------+--------------+--------------------------------------------------------+
| 2. HOST (Variable)                                                                               |
|   host_mode              | 2            | 0 = Dict Host, 1 = Base26-lower, 2 = Base62/Mixed      |
|   [Mode 0] host_index    | 8            | 0..=39 (COMMON_HOSTS), 255 = Escape to Literal         |
|   [Mode 1/2] segment     | Variable     | alphabet_id(4) + symbol_count(varint) + ...            |
+--------------------------+--------------+--------------------------------------------------------+
| 3. TLD (Variable)                                                                                |
|   tld_mode               | 1            | 0 = Dict TLD, 1 = Literal Base26-lower                 |
|   [Mode 0] tld_index     | 5            | 0..=30 (KNOWN_TLDS), 31 = EMPTY TLD (IP/Bare host)     |
|   [Mode 1] segment       | Variable     | alphabet_id(4) + symbol_count(varint) + ...            |
+--------------------------+--------------+--------------------------------------------------------+
| 4. INDEX-SUFFIX LITERAL (Optional, only present if header.index_suffix == 3)                      |
|   literal_segment        | Variable     | Segment encoding for custom "index.*" filename         |
+--------------------------+--------------+--------------------------------------------------------+
| 5. RESOURCE REGIONS (Variable)                                                                   |
|   path_present           | 1            | 1 = Path segments exist, 0 = None                      |
|   query_present          | 1            | 1 = Query pairs exist, 0 = None                        |
|   fragment_present       | 1            | 1 = Fragment segments exist, 0 = None                  |
|   -- For each present region:                                                                    |
|     segment_count        | varint       | Number of segments in this region (<= 64)              |
|     segments             | Variable     | N repetitions of Segment Layout                        |
+--------------------------+--------------+--------------------------------------------------------+
| SEGMENT LAYOUT (Repeated for every segment)                                                      |
|   alphabet_id            | 4            | 0..=4 (Base-N), 5 (Huffman), 6 (Raw bytes)             |
|   symbol_count           | varint       | Number of original characters / symbols (<= 4096)      |
|   value_bit_length       | varint       | Bit width W of following payload                       |
|   value_bits             | W            | Big-endian value bits (MSB-first)                      |
+--------------------------+--------------+--------------------------------------------------------+
| 6. TAIL PADDING                                                                                  |
|   zero_pad               | 0..7         | Zero-bits to align bitstream to whole byte boundary    |
+--------------------------+--------------+--------------------------------------------------------+
```

---

## 4.8 `decode.rs` — Bitstream Deserializer & Resilient URL Synthesizer

### Defensive Resource Boundaries & Safety Guardrails

```rust
pub(crate) const MAX_PAYLOAD_BYTES: usize = 65_536;    // Max payload string length
pub(crate) const MAX_SEGMENT_COUNT: u64 = 64;          // Max segments per region
pub(crate) const MAX_SYMBOL_COUNT: u64 = 4_096;        // Max characters per segment
pub(crate) const MAX_VALUE_BIT_LENGTH: u64 = 65_536;   // Max bit length per segment value
const MAX_HOST_TLD_LEN: usize = 4_096;                 // Max length for host/tld strings
```

### Mandatory Validation Rules

| Check | Error Condition | Rationale |
|---|---|---|
| `version == 1` | `Error::UnsupportedVersion` | Prevents interpreting future format revisions as v1 |
| `dict_set_id == 1` | `Error::InvalidPayload("unknown dict set")` | Prevents decoding against mismatched codebook dictionaries |
| Bounded bitstream read | `Error::InvalidPayload` | Prevents panicking on truncated or incomplete byte payloads |
| Trailing zero-padding | `Error::InvalidPayload("non-zero padding")` | Ensures bitstream integrity and rejects malformed tails |
| `segment_count <= 64` per region | `Error::InvalidPayload("segment count too large")` | Prevents memory allocation DoS attacks |
| `symbol_count <= 4096` per segment | `Error::InvalidPayload("symbol count too large")` | Bounds single-segment allocation sizes |
| `payload_len <= 65536` bytes | `Error::InvalidPayload("payload too large")` | Hard upper boundary on incoming text strings |
| `host_index <= 39` or `255` | `Error::InvalidPayload("host index out of range")` | Memory safety over static array slices |
| `tld_index <= 31` | `Error::InvalidPayload("tld index out of range")` | Bounds TLD dictionary lookup |
| `alphabet_id <= 6` (7 reserved) | `Error::InvalidPayload("reserved alphabet id")` | Strict rejection of unassigned alphabet slots |
| Huffman decoded count == `symbol_count` | `Error::HuffmanError` | Verifies prefix codebook bitstream integrity |

### Edge Cases Matrix

| Case | Encoder Behavior | Decoder Behavior |
|---|---|---|
| **Unsupported character** (outside alphabets 0–4) | Uses `alphabet_id = 6` (raw-fallback), writes UTF-8 bytes | Reconstructs raw UTF-8 bytes losslessly |
| **Unknown TLD** (not in `KNOWN_TLDS`) | `tld_mode = 1`, encodes literal TLD as Base26-lower | Decodes Base26 literal string as TLD |
| **Mixed-case path segment** | `analyze_segment()` selects Base62 (`id = 3`) or Base64url (`id = 4`) | Decodes using declared `alphabet_id` |
| **Non-ASCII Unicode URL** | URL parser normalizes to uppercase percent-escapes (`%E6%97%A5`) | Preserves percent-encoding in reconstructed URL |
| **Corrupt / Flipped bits** | N/A | Strictly returns `Err(Error::InvalidPayload)` without panicking |
| **Empty path/query/fragment** | Sets region flag to `0`, writing 0 bits for that region | Region flag `0` initializes empty vector `[]` |
| **Host = "localhost" / Bare IPv4** | `tld_mode = 0`, `tld_index = 31` (`TLD_ESCAPE`), host encoded as-is | Reconstructs dot-less host with empty TLD `""` |
| **Custom Index Suffix** (e.g. `index.aspx`) | `index_suffix = 3` (Other), followed by literal segment | Decodes literal segment, reconstructs filename |


---

## 4.9 `error.rs` — Strongly-Typed Error Architecture

```rust
#[derive(Error, Debug)]
pub enum Error {
    #[error("invalid url: {reason}")]
    InvalidUrl { reason: String },

    #[error("invalid payload: {reason}")]
    InvalidPayload { reason: String },

    #[error("unsupported version: {0}")]
    UnsupportedVersion(u8),

    #[error("unsupported character: {0}")]
    UnsupportedCharacter(char),

    #[error("huffman error: {reason}")]
    HuffmanError { reason: String },

    #[error(transparent)]
    Io(#[from] std::io::Error),
}
```

---

## 4.10 `crates/urlz/src/main.rs` — CLI Dispatcher & Ergonomics

```
urlz
├── encode <URL>                             # Encodes URL -> Base85 payload to stdout
├── decode <PAYLOAD>                         # Decodes Base85 payload -> URL to stdout
└── dict build [--corpus <FILE>] --out <DIR> # Generates codebook.bin from URL training corpus
```

---

# Module 5: End-to-End Execution Paths & Sequence Traces

---

## 5.1 Trace 1: Standard URL Encoding to Base85 Payload (`encode::encode`)

```
User Input: "https://www.google.com/search?q=rust"
  │
  ├─► [1] main::run()
  │     └─► clap matches Command::Encode { url }
  │
  ├─► [2] urlz::encode::encode(url)
  │     │
  │     ├─► [2.1] urlparse::parse_url("https://www.google.com/search?q=rust")
  │     │     ├─► normalize_percent_encoding() -> "https://www.google.com/search?q=rust"
  │     │     ├─► Extract scheme: https -> https = true
  │     │     ├─► Extract authority: "www.google.com"
  │     │     │     ├─► Strip "www." -> www = true, host_label = "google.com"
  │     │     │     └─► Split at '.' -> host = "google", tld = "com"
  │     │     ├─► Extract path: "/search" -> path_segments = ["search"]
  │     │     ├─► Extract query: "q=rust" -> query_segments = [("q", Some("rust"))]
  │     │     └─► Returns ParsedUrl struct
  │     │
  │     ├─► [2.2] Write Header (12 bits) to WriteBitStream
  │     │     ├─► write_bits(version = 1, 4)
  │     │     ├─► write_bits(dict_set_id = 1, 4)
  │     │     ├─► write_bits(https = 1, 1)
  │     │     ├─► write_bits(www = 1, 1)
  │     │     └─► write_bits(index_suffix = 0, 2)
  │     │
  │     ├─► [2.3] Write Host
  │     │     ├─► dict::lookup_host("google") -> Some(0)
  │     │     ├─► write_bits(host_mode = 0, 2)
  │     │     └─► write_bits(host_index = 0, 8)
  │     │
  │     ├─► [2.4] Write TLD
  │     │     ├─► dict::lookup_tld("com") -> Some(0)
  │     │     ├─► write_bits(tld_mode = 0, 1)
  │     │     └─► write_bits(tld_index = 0, 5)
  │     │
  │     ├─► [2.5] Write Resource Presence Flags (3 bits)
  │     │     ├─► path_present = 1 (1 bit)
  │     │     ├─► query_present = 1 (1 bit)
  │     │     └─► fragment_present = 0 (1 bit)
  │     │
  │     ├─► [2.6] Write Path Region
  │     │     ├─► write_varint(segment_count = 1)
  │     │     ├─► segment::analyze_segment("search") -> alphabet_id = 1 (base26-lower)
  │     │     └─► write_segment(): evaluates Huffman vs Base26 cost -> writes chosen bits
  │     │
  │     ├─► [2.7] Write Query Region
  │     │     ├─► write_varint(segment_count = 1)
  │     │     ├─► Segment string: "q=rust"
  │     │     ├─► segment::analyze_segment("q=rust") -> alphabet_id = 6 (raw, '=' present)
  │     │     └─► write_segment(): Huffman wins over raw -> writes alphabet_id = 5 + Huffman bits
  │     │
  │     ├─► [2.8] Finalize Bitstream
  │     │     ├─► WriteBitStream::into_bytes() -> Flushes zero-padding to byte boundary
  │     │     └─► Produces raw bytes Vec<u8>
  │     │
  │     ├─► [2.9] Base85 Radix Conversion
  │     │     ├─► alphabet::biguint_from_bytes_be(&bytes) -> BigUint N
  │     │     └─► alphabet::to_base(&N, BASE85_ALPHABET) -> String
  │     │
  │     └─► [2.10] Validate payload.len() <= 65536
  │
  └─► [3] Prints Base85 payload to stdout
```

---

## 5.2 Trace 2: Decoding & Validation (`decode::decode`)

```
User Input: Encoded Base85 Payload String S
  │
  ├─► [1] decode::decode(payload)
  │     ├─► Check payload.len() <= 65536
  │     ├─► from_base(payload, BASE85_ALPHABET) -> BigUint N
  │     ├─► bytes_from_biguint_be(&N) -> Vec<u8> bits
  │     │
  │     └─► [2] decode_bits(&bits)
  │           ├─► ReadBitStream::from_bytes(&bits)
  │           ├─► Read Header (12b): version == 1, dict_set_id == 1, https, www, index_code
  │           ├─► Read Host (host_mode 0, 1, or 2) -> Lookup dict or decode segment
  │           ├─► Read TLD (tld_mode 0 or 1) -> Lookup dict or decode segment
  │           ├─► Read Index Suffix (if index_code == 3, decode literal segment)
  │           ├─► Read Region Flags (path_present, query_present, fragment_present)
  │           ├─► Read Segments:
  │           │     ├─► Read alphabet_id (4 bits)
  │           │     ├─► Read symbol_count (varint) <= 4096
  │           │     ├─► Read value_bit_length (varint) <= 65536
  │           │     ├─► If alphabet_id == 5: HuffmanDecoder::decode()
  │           │     └─► Else: read_biguint_bits() -> biguint_to_symbols() (left-pad with alphabet[0])
  │           ├─► Validate Trailing Padding: read_remaining_all_zero() must be true
  │           └─► Assemble and return normalized URL string
  │
  └─► [3] Return Ok(url)
```

---

## 5.3 Trace 3: Codebook Training & Building from URL Corpus (`urlz dict build`)

```
User Invocation: `urlz dict build corpus.txt --out dictionaries/v1`
  │
  ├─► [1] main::run()
  │     └─► Command::Dict { command: DictCommand::Build { corpus, out } }
  │
  ├─► [2] Read Corpus File: std::fs::read_to_string("corpus.txt")
  │
  ├─► [3] huffman::parse_corpus(&text)
  │     ├─► Iterates over each line
  │     ├─► Strips scheme and authority (everything up to first '/' after '://')
  │     └─► Concatenates remaining path, query, and fragment characters
  │
  ├─► [4] huffman::build_from_corpus(&parsed_text)
  │     ├─► Frequency Array: freqs = [0u64; 256]
  │     ├─► For each byte b in parsed_text:
  │     │     └─► If b in BASE85_ALPHABET: freqs[b] += 1
  │     │
  │     └─► huffman::build_codebook(&freqs)
  │           ├─► Populate BinaryHeap<HeapNode>
  │           ├─► Iteratively pop 2 lowest nodes, merge into parent, push back
  │           ├─► DFS traverse from root to record leaf depths
  │           └─► Returns Codebook([u8; 256])
  │
  ├─► [5] Create Output Directory: std::fs::create_dir_all(&out)
  ├─► [6] Write Binary Codebook: huffman::write_codebook_file("out/codebook.bin", &cb)
  │     └─► Serializes exactly 256 raw length bytes
  │
  └─► [7] Outputs confirmation to stdout with byte size (256 bytes)
```

---

## 5.4 Trace 4: Adversarial / Corrupt Payload Rejection & Recovery

```
Attacker Input: Truncated / Bit-Flipped Payload String S
  │
  ├─► [1] decode::decode(S)
  │
  ├─► [Check 1] Payload Length Bomb: len > 65536 -> Err(Error::InvalidPayload)
  │
  ├─► [Check 2] Character Validation: Char not in Base85 -> Err(Error::UnsupportedCharacter)
  │
  ├─► [Check 3] Bitstream Header Checks:
  │     ├─► Version != 1 -> Err(Error::UnsupportedVersion)
  │     └─► DictSetID != 1 -> Err(Error::InvalidPayload("unknown dict set"))
  │
  ├─► [Check 4] Resource Bounds:
  │     ├─► Varint overflow (> 10 groups) -> Err(Error::InvalidPayload)
  │     ├─► Segment count > 64 -> Err(Error::InvalidPayload("segment count too large"))
  │     └─► Symbol count > 4096 -> Err(Error::InvalidPayload("symbol count too large"))
  │
  ├─► [Check 5] Dictionary Range Check:
  │     └─► Host Index >= 40 (and != 255) -> Err(Error::InvalidPayload) [No panic]
  │
  ├─► [Check 6] Huffman Stream Integrity:
  │     ├─► Truncated bitstream -> Err(Error::HuffmanError)
  │     └─► Invalid prefix code -> Err(Error::HuffmanError)
  │
  └─► [Check 7] Padding Validation:
        └─► Trailing bits contain a '1' -> Err(Error::InvalidPayload("non-zero padding"))
```

---

# Module 6: Design Patterns & Performance

---

## 6.1 Design Patterns Implemented

| Design Pattern | Concrete Location in Codebase | Problem Solved & Structural Value |
|---|---|---|
| **Strategy Pattern** | `segment::analyze_segment` & `segment::ALPHABETS` | Encapsulates distinct radix conversion algorithms (Base10 through Base64url) behind uniform `SegmentEncoding` interfaces, allowing dynamic algorithm selection per segment. |
| **Self-Delimiting Serializer** | `encode::encode_to_bits` / `decode::decode_bits` | Avoids explicit framing bytes or end-of-payload sentinels by embedding bit lengths and varints directly into the data stream, enabling exact bit consumption validation. |
| **Arena & Min-Heap Synthesis** | `huffman::build_codebook` | Constructs canonical binary trees in a flat vector arena (`Vec<(u64, u64, Option<usize>, Option<usize>)>`), eliminating dynamic pointer heap thrashing. |
| **Value Object / Newtype** | `huffman::Codebook` (`[u8; 256]`) | Enforces type safety, copy semantics, and serialization invariants over raw array buffers. |

---

## 6.2 Performance & Benchmarks

### Macro-Benchmarks across Tranco Datasets:
| Metric | Value |
| :--- | :--- |
| **Throughput** | **143,844 URLs/second** |
| **Mean Latency** | **6.95 µs / URL** |
| **Encode Errors** | **0 (100% lossless fidelity)** |
| **Average Compression** | **1.21× (17.5% wire size reduction)** |

### Per-URL Stateless Compression vs. General-Purpose Algorithms:
When compressing individual URLs in isolation (for QR codes, BLE beacons, SMS, or cache payloads), general-purpose algorithms expand payload size due to header overhead and sliding-window startup costs:

| Algorithm | Wire Format | Ratio (`src/enc`) on 200k URLs | Behavior on Short Strings |
| :--- | :---: | :---: | :--- |
| **`urlz`** | **Base85** | **1.212× (17.5% smaller)** | **Lossless Compression** |
| **Raw DEFLATE (Level 9)** | Base85 | 0.852× (17.4% larger) | Negative Compression (Expansion) |
| **`zlib`** | Base85 | 0.795× (25.8% larger) | Negative Compression (Expansion) |
| **`gzip`** | Base85 | 0.697× (43.5% larger) | Negative Compression (Expansion) |


