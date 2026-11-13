use thiserror::Error;

#[derive(Error, Debug)]
pub enum QueryError {
    #[error("core error: {0}")]
    Core(#[from] imgseek_core::CoreError),

    #[error("store error: {0}")]
    Store(#[from] imgseek_store::StoreError),

    #[error("index error: {0}")]
    Index(#[from] imgseek_index::IndexError),

    #[error("features error: {0}")]
    Features(#[from] imgseek_features::FeaturesError),

    #[error("candidate count {0} exceeded safety limit")]
    CandidateLimitExceeded(usize),

    #[error("invalid query parameter: {0}")]
    InvalidQuery(String),

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
}

pub type Result<T> = std::result::Result<T, QueryError>;

