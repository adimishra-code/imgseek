use thiserror::Error;

#[derive(Error, Debug)]
pub enum ChromeError {
    #[error("core error: {0}")]
    Core(#[from] imgseek_core::CoreError),

    #[error("store error: {0}")]
    Store(#[from] imgseek_store::StoreError),

    #[error("index error: {0}")]
    Index(#[from] imgseek_index::IndexError),

    #[error("Chrome profile not found: {0}")]
    ProfileNotFound(String),

    #[error("History database is locked by Chrome: {0}")]
    LockedDatabase(String),

    #[error("corrupt Simple Cache entry: {0}")]
    CorruptCacheEntry(String),

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
}

pub type Result<T> = std::result::Result<T, ChromeError>;
