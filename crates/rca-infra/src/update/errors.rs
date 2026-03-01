use thiserror::Error;

#[derive(Debug, Error)]
pub enum UpdateError {
    #[error("request failed: {0}")]
    Request(#[from] reqwest::Error),

    #[error("unexpected status: {0}")]
    UnexpectedStatus(reqwest::StatusCode),

    #[error("invalid release tag version: {0}")]
    InvalidReleaseTag(String),

    #[error("invalid current version: {0}")]
    InvalidCurrentVersion(String),

    #[error("invalid published_at timestamp: {0}")]
    InvalidPublishedAt(#[from] chrono::ParseError),
}
