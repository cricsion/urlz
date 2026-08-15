//! Error types shared by URL parsing, encoding, decoding, and Huffman compression.

use thiserror::Error;

/// All fallible operations in this crate return `Result<T, Error>`.
///
/// Marked `#[non_exhaustive]`: new variants may be added in minor releases.
/// Variants carrying a `reason` field hold human-readable diagnostic strings;
/// they are not stable match targets — match on the variant, never the message.
#[derive(Error, Debug)]
#[non_exhaustive]
pub enum Error {
    #[error("invalid url: {reason}")]
    InvalidUrl { reason: String },

    #[error("invalid payload: {reason}")]
    InvalidPayload { reason: String },

    #[error("unsupported format version: {0}")]
    UnsupportedVersion(u8),

    #[error("unsupported character: {0}")]
    UnsupportedCharacter(char),

    #[error("huffman error: {reason}")]
    HuffmanError { reason: String },

    #[error(transparent)]
    Io(#[from] std::io::Error),
}
