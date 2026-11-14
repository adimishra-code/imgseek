pub mod chroma;
pub mod composite;
pub mod config;
pub mod decode;
pub mod error;
pub mod format;
pub mod hash;
pub mod normalize;
pub mod orient;
pub mod scoring;

pub use chroma::{
    bytes_to_chroma, chroma_to_bytes, compute_color_histogram, histogram_distance,
    histogram_intersection, hsv_to_bin, rgb_to_hsv,
};
pub use composite::composite_onto_white;
pub use config::Config;
pub use decode::decode_image;
pub use error::{CoreError, Result};
pub use format::{sniff_format, ImageFormat};
pub use hash::{
    ahash_from_8x8, ahash_from_luma, compute_pdq_quality, dhash_from_9x8, dhash_from_luma,
    pdq_from_luma, pdq_from_matrix_64x64, phash_from_32x32, phash_from_luma, whash_from_64x64,
    whash_from_luma, PdqHash,
};
pub use normalize::{normalize, rgb_to_luma_byte, AnalysisView, NormalizedImage};
pub use orient::{apply_orientation, parse_exif_orientation, ExifOrientation};
pub use scoring::{compute_combined_score, ScoreBreakdown};
