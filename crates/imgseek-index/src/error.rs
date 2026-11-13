use thiserror::Error;

#[derive(Error, Debug)]
pub enum IndexError {
    #[error("core error: {0}")]
    Core(#[from] imgseek_core::CoreError),

    #[error("corrupt segment: {0}")]
    CorruptSegment(String),

    #[error("checksum mismatch: expected {expected:#010x}, found {actual:#010x}")]
    ChecksumMismatch { expected: u32, actual: u32 },

    #[error("format version mismatch: expected {expected}, found {actual}")]
    VersionMismatch { expected: u32, actual: u32 },

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
}

pub type Result<T> = std::result::Result<T, IndexError>;

