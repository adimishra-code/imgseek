use thiserror::Error;

#[derive(Error, Debug)]
pub enum StoreError {
    #[error("core error: {0}")]
    Core(#[from] imgseek_core::CoreError),

    #[error("database error: {0}")]
    Database(String),

    #[error("blob not found: {0}")]
    BlobNotFound(String),

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
}

pub type Result<T> = std::result::Result<T, StoreError>;

