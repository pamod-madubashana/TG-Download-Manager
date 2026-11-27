use std::time::Duration;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum FetchError {
    #[error("flood wait: {0:?}")]
    FloodWait(Duration),
    #[error("file reference expired")]
    FileRefExpired,
    #[error("transient: {0}")]
    Transient(String),
    #[error("fatal: {0}")]
    Fatal(String),
}

#[derive(Debug, Error)]
pub enum DownloadError {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("config: {0}")]
    Config(String),
    #[error("source: {0}")]
    Source(#[from] FetchError),
    #[error("verify: {0}")]
    Verify(String),
    #[error("telegram: {0}")]
    Telegram(String),
    #[error("cancelled")]
    Cancelled,
}
