use thiserror::Error;

#[derive(Error, Debug)]
pub enum CrawlError {
    #[error("core error: {0}")]
    Core(#[from] imgseek_core::CoreError),

    #[error("SSRF guard blocked request to {0}")]
    SsrfBlocked(String),

    #[error("URL disallowed by robots.txt: {0}")]
    RobotsDisallowed(String),

    #[error("rate limited by host: {0}")]
    RateLimited(String),

    #[error("download cap exceeded: {0} bytes")]
    DownloadCapExceeded(u64),

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
}

pub type Result<T> = std::result::Result<T, CrawlError>;
