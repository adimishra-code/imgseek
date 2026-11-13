//! Core types, errors, normalisation, hashes, and configuration.

pub mod config;
pub mod error;

pub use config::Config;
pub use error::{CoreError, Result};
