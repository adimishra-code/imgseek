use thiserror::Error;

#[derive(Error, Debug)]
pub enum WebError {
    #[error("core error: {0}")]
    Core(#[from] imgseek_core::CoreError),

    #[error("query error: {0}")]
    Query(#[from] imgseek_query::QueryError),

    #[error("unauthorized request origin: {0}")]
    UnauthorizedOrigin(String),

    #[error("invalid Host header: {0}")]
    InvalidHost(String),

    #[error("upload payload size {0} bytes exceeds maximum allowed")]
    PayloadTooLarge(usize),

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
}

pub type Result<T> = std::result::Result<T, WebError>;
