pub mod dict;
pub mod error;
pub use error::Error;
pub type Result<T, E = Error> = std::result::Result<T, E>;
