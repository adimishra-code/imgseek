use thiserror::Error;

#[derive(Error, Debug)]
pub enum FeaturesError {
    #[error("core error: {0}")]
    Core(#[from] imgseek_core::CoreError),

    #[error("invalid visual vocabulary: {0}")]
    InvalidVocabulary(String),

    #[error("degenerate geometric model: {0}")]
    DegenerateModel(String),

    #[error("RANSAC estimation failed: {0}")]
    RansacFailed(String),

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
}

pub type Result<T> = std::result::Result<T, FeaturesError>;

