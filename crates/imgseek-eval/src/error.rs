use thiserror::Error;

#[derive(Error, Debug)]
pub enum EvalError {
    #[error("core error: {0}")]
    Core(#[from] imgseek_core::CoreError),

    #[error("query error: {0}")]
    Query(#[from] imgseek_query::QueryError),

    #[error("index error: {0}")]
    Index(#[from] imgseek_index::IndexError),

    #[error("store error: {0}")]
    Store(#[from] imgseek_store::StoreError),

    #[error("dataset not found: {0}")]
    DatasetNotFound(String),

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
}

pub type Result<T> = std::result::Result<T, EvalError>;
