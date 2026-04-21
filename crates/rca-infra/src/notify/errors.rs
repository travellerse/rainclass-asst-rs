use thiserror::Error;

#[derive(Debug, Error)]
pub enum NotifyError {
    #[error("backend unavailable: {0}")]
    BackendUnavailable(String),

    #[error("platform error: {0}")]
    Platform(String),

    #[error("send failed: {0}")]
    SendFailed(String),

    #[error("insecure URL: {0}")]
    InsecureUrl(String),

    #[error("private network not allowed: {0}")]
    PrivateNetworkNotAllowed(String),
}
