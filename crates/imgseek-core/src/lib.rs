//! Core types, errors, normalisation, hashes, and configuration.

pub mod config;
pub mod error;
pub mod format;

pub use config::Config;
pub use error::{CoreError, Result};
pub use format::{sniff_format, ImageFormat};
