use thiserror::Error;

#[derive(Debug, Error)]
pub enum NotifyError {
    #[error("backend unavailable: {0}")]
    BackendUnavailable(String),

    #[error("platform error: {0}")]
    Platform(String),
}
