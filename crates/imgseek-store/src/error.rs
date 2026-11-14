use thiserror::Error;

#[derive(Error, Debug)]
pub enum StoreError {
    #[error("core error: {0}")]
    Core(#[from] imgseek_core::CoreError),

    #[error("database error: {0}")]
    Database(#[from] rusqlite::Error),

    #[error("blob not found: {0}")]
    BlobNotFound(String),

    #[error("blob corrupted: expected {expected}, got {actual}")]
    BlobCorrupted { expected: String, actual: String },

    #[error("invalid blob hash: {0}")]
    InvalidBlobHash(String),

    #[error("schema error: {0}")]
    Schema(String),

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
}

pub type Result<T> = std::result::Result<T, StoreError>;
