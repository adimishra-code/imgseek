//! Core types, errors, normalisation, hashes, and configuration.

pub mod config;
pub mod decode;
pub mod error;
pub mod format;

pub use config::Config;
pub use decode::decode_image;
pub use error::{CoreError, Result};
pub use format::{sniff_format, ImageFormat};
