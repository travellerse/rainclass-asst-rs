use thiserror::Error;

use crate::app::ports::{ApiPortError, StoragePortError};

#[derive(Debug, Error)]
pub enum AuthError {
    #[error("api error: {0}")]
    Api(#[from] ApiPortError),

    #[error("storage error: {0}")]
    Storage(#[from] StoragePortError),

    #[error("session missing")]
    SessionMissing,

    #[error("session expired")]
    SessionExpired,

    #[error("invalid auth state: {0}")]
    InvalidState(String),
}
