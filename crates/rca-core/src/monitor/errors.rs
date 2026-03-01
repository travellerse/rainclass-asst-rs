use thiserror::Error;

use crate::app::ports::ApiPortError;
use crate::auth::AuthError;
use crate::domain::DomainError;

#[derive(Debug, Error)]
pub enum MonitorError {
    #[error("auth error: {0}")]
    Auth(#[from] AuthError),

    #[error("api error: {0}")]
    Api(#[from] ApiPortError),

    #[error("domain error: {0}")]
    Domain(#[from] DomainError),

    #[error("internal channel closed")]
    ChannelClosed,

    #[error("task join error: {0}")]
    Join(String),

    #[error("already started")]
    AlreadyStarted,

    #[error("not running")]
    NotRunning,
}
