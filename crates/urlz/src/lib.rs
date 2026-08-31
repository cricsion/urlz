//! urlz — a high-efficiency URL compression library and CLI tool.
//!
//! urlz compresses URLs into compact payloads using a canonical Huffman
//! code over a custom alphabet, backed by TLD/host dictionaries.
//!
//! # Examples
//!
//! ```
//! let payload = urlz::encode("https://github.com/rust-lang/rust")?;
//! assert_eq!(urlz::decode(&payload)?, "https://github.com/rust-lang/rust");
//! # Ok::<(), urlz::Error>(())
//! ```

pub mod alphabet;
pub mod bitstream;
pub mod decode;
pub mod dict;
pub mod encode;
pub mod error;
pub mod huffman;
pub mod segment;
pub mod urlparse;

pub use decode::{decode, decode_bits};
pub use encode::{encode, encode_to_bits};
pub use error::Error;
pub use huffman::{Codebook, default_codebook};
pub use urlparse::{IndexSuffix, ParsedUrl, parse_url};

/// A specialized [`Result`] type for `urlz` operations.
pub type Result<T, E = Error> = std::result::Result<T, E>;
