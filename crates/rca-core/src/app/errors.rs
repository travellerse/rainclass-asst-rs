use thiserror::Error;

use crate::app::ports::{ApiPortError, NotifyPortError, StoragePortError, UpdatePortError};
use crate::auth::AuthError;
use crate::monitor::MonitorError;

#[derive(Debug, Error)]
pub enum AppError {
    #[error("api port error: {0}")]
    ApiPort(#[from] ApiPortError),

    #[error("storage port error: {0}")]
    StoragePort(#[from] StoragePortError),

    #[error("notify port error: {0}")]
    NotifyPort(#[from] NotifyPortError),

    #[error("update port error: {0}")]
    UpdatePort(#[from] UpdatePortError),

    #[error("auth error: {0}")]
    Auth(#[from] AuthError),

    #[error("monitor error: {0}")]
    Monitor(#[from] MonitorError),

    #[error("invalid command: {0}")]
    InvalidCommand(String),

    #[error("unexpected state: {0}")]
    UnexpectedState(String),
}
