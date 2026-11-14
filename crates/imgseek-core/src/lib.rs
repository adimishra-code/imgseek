//! Core types, errors, normalisation, hashes, and configuration.

pub mod composite;
pub mod config;
pub mod decode;
pub mod error;
pub mod format;
pub mod hash;
pub mod normalize;
pub mod orient;

pub use composite::composite_onto_white;
pub use config::Config;
pub use decode::decode_image;
pub use error::{CoreError, Result};
pub use format::{sniff_format, ImageFormat};
pub use hash::{ahash_from_8x8, ahash_from_luma};
pub use normalize::{normalize, rgb_to_luma_byte, AnalysisView, NormalizedImage};
pub use orient::{apply_orientation, parse_exif_orientation, ExifOrientation};
