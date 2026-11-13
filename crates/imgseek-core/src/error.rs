use thiserror::Error;

#[derive(Error, Debug)]
pub enum CoreError {
    #[error("unsupported image format: {0}")]
    UnsupportedFormat(String),

    #[error("image dimensions {width}x{height} exceed pixel budget of {limit} pixels")]
    PixelBudgetExceeded { width: u32, height: u32, limit: u64 },

    #[error("image decoding failed: {0}")]
    DecodeError(String),

    #[error("configuration error: {0}")]
    ConfigError(String),

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
}

pub type Result<T> = std::result::Result<T, CoreError>;
