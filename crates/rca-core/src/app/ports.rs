use async_trait::async_trait;
use thiserror::Error;
use tokio::sync::mpsc;

use crate::app::{AppConfigDto, AppNotification};
use crate::auth::{AuthSession, QrLoginBootstrap, QrLoginProgress};
use crate::domain::{AnswerPayload, CheckinId, Lesson, LessonId, Problem, ProblemId};

#[derive(Debug, Clone)]
pub enum LessonWsEvent {
    ProblemPublished {
        problem: Problem,
    },
    CheckinOpened {
        checkin_id: CheckinId,
    },
    LessonEnded,
    DanmuPublished {
        user_name: Option<String>,
        content: String,
    },
    CallPaused {
        target_name: String,
    },
    PresentationUpdated {
        presentation_id: u64,
    },
    SlideNavigated {
        presentation_id: u64,
        slide_id: u64,
        slide_index: u64,
    },
    ProblemUnlocked {
        problem_id: ProblemId,
    },
    Warning {
        message: String,
    },
    Unknown {
        raw_type: String,
    },
}

#[derive(Debug, Clone)]
pub struct UpdateInfo {
    pub latest_version: String,
    pub release_url: String,
    pub published_at_unix_ms: i64,
}

#[derive(Debug, Clone, Error)]
pub enum ApiPortError {
    #[error("api request failed ({context}): {detail}")]
    RequestFailed {
        context: &'static str,
        detail: String,
    },

    #[error("api request timed out: {0}")]
    Timeout(String),

    #[error("api protocol changed: {0}")]
    ProtocolChanged(String),

    #[error("lesson ended")]
    LessonEnded,

    #[error("api auth failed: {0}")]
    AuthFailed(String),
}

impl ApiPortError {
    pub fn request(context: &'static str, detail: impl ToString) -> Self {
        Self::RequestFailed {
            context,
            detail: detail.to_string(),
        }
    }

    pub fn timeout(detail: impl ToString) -> Self {
        Self::Timeout(detail.to_string())
    }

    pub fn protocol(detail: impl ToString) -> Self {
        Self::ProtocolChanged(detail.to_string())
    }

    pub fn auth(detail: impl ToString) -> Self {
        Self::AuthFailed(detail.to_string())
    }
}

#[derive(Debug, Clone, Error)]
pub enum StoragePortError {
    #[error("storage load failed: {0}")]
    LoadFailed(String),

    #[error("storage save failed: {0}")]
    SaveFailed(String),

    #[error("storage clear failed: {0}")]
    ClearFailed(String),
}

impl StoragePortError {
    pub fn load(detail: impl ToString) -> Self {
        Self::LoadFailed(detail.to_string())
    }

    pub fn save(detail: impl ToString) -> Self {
        Self::SaveFailed(detail.to_string())
    }

    pub fn clear(detail: impl ToString) -> Self {
        Self::ClearFailed(detail.to_string())
    }
}

#[derive(Debug, Clone, Error)]
pub enum NotifyPortError {
    #[error("notify send failed: {0}")]
    SendFailed(String),
}

impl NotifyPortError {
    pub fn send(detail: impl ToString) -> Self {
        Self::SendFailed(detail.to_string())
    }
}

#[derive(Debug, Clone, Error)]
pub enum UpdatePortError {
    #[error("update check failed: {0}")]
    CheckFailed(String),
}

impl UpdatePortError {
    pub fn check(detail: impl ToString) -> Self {
        Self::CheckFailed(detail.to_string())
    }
}

#[async_trait]
pub trait ApiPort: Send + Sync {
    async fn get_on_lessons(&self, session: &AuthSession) -> Result<Vec<Lesson>, ApiPortError>;
    async fn get_lesson_problems(
        &self,
        session: &AuthSession,
        lesson_id: LessonId,
    ) -> Result<Vec<Problem>, ApiPortError>;
    async fn submit_answer(
        &self,
        session: &AuthSession,
        lesson_id: LessonId,
        problem_id: ProblemId,
        payload: AnswerPayload,
    ) -> Result<(), ApiPortError>;
    async fn submit_checkin(
        &self,
        session: &AuthSession,
        lesson_id: LessonId,
        checkin_id: CheckinId,
    ) -> Result<(), ApiPortError>;
    async fn send_danmu(
        &self,
        session: &AuthSession,
        lesson_id: LessonId,
        content: &str,
    ) -> Result<(), ApiPortError>;
    async fn report_page_view(
        &self,
        session: &AuthSession,
        lesson: &Lesson,
        slide_index: u64,
    ) -> Result<(), ApiPortError>;
    async fn start_qr_login(&self) -> Result<QrLoginBootstrap, ApiPortError>;
    async fn poll_qr_login(&self, scene_id: &str) -> Result<QrLoginProgress, ApiPortError>;
    async fn wait_qr_login(
        &self,
        scene_id: &str,
        timeout_secs: u64,
    ) -> Result<QrLoginProgress, ApiPortError>;
    async fn refresh_session(&self, refresh_token: &str) -> Result<AuthSession, ApiPortError>;
    async fn connect_lesson_stream(
        &self,
        session: &AuthSession,
        lesson_id: LessonId,
    ) -> Result<mpsc::Receiver<LessonWsEvent>, ApiPortError>;
    async fn download_presentation(
        &self,
        session: &AuthSession,
        presentation_id: u64,
        lesson_id: Option<u64>,
        save_dir: &std::path::Path,
    ) -> Result<std::path::PathBuf, ApiPortError>;
}

#[async_trait]
pub trait SessionStorePort: Send + Sync {
    async fn load_session(&self) -> Result<Option<AuthSession>, StoragePortError>;
    async fn save_session(&self, session: &AuthSession) -> Result<(), StoragePortError>;
    async fn clear_session(&self) -> Result<(), StoragePortError>;
}

#[async_trait]
pub trait ConfigStorePort: Send + Sync {
    async fn load_config(&self) -> Result<AppConfigDto, StoragePortError>;
    async fn save_config(&self, config: &AppConfigDto) -> Result<(), StoragePortError>;
}

#[async_trait]
pub trait NotifierPort: Send + Sync {
    async fn notify(&self, message: AppNotification) -> Result<(), NotifyPortError>;
}

#[async_trait]
pub trait UpdateCheckerPort: Send + Sync {
    async fn check_latest(
        &self,
        current_version: &str,
    ) -> Result<Option<UpdateInfo>, UpdatePortError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_api_port_error_constructors() {
        let err = ApiPortError::request("login", "network error");
        match err {
            ApiPortError::RequestFailed { context, detail } => {
                assert_eq!(context, "login");
                assert_eq!(detail, "network error");
            }
            _ => panic!("Expected ApiPortError::RequestFailed"),
        }

        let err = ApiPortError::timeout("connection timeout");
        match err {
            ApiPortError::Timeout(detail) => {
                assert_eq!(detail, "connection timeout");
            }
            _ => panic!("Expected ApiPortError::Timeout"),
        }

        let err = ApiPortError::protocol("unsupported version");
        match err {
            ApiPortError::ProtocolChanged(detail) => {
                assert_eq!(detail, "unsupported version");
            }
            _ => panic!("Expected ApiPortError::ProtocolChanged"),
        }

        let err = ApiPortError::auth("invalid token");
        match err {
            ApiPortError::AuthFailed(detail) => {
                assert_eq!(detail, "invalid token");
            }
            _ => panic!("Expected ApiPortError::AuthFailed"),
        }
    }

    #[test]
    fn test_storage_port_error_constructors() {
        let err = StoragePortError::load("file not found");
        match err {
            StoragePortError::LoadFailed(detail) => {
                assert_eq!(detail, "file not found");
            }
            _ => panic!("Expected StoragePortError::LoadFailed"),
        }

        let err = StoragePortError::save("disk full");
        match err {
            StoragePortError::SaveFailed(detail) => {
                assert_eq!(detail, "disk full");
            }
            _ => panic!("Expected StoragePortError::SaveFailed"),
        }

        let err = StoragePortError::clear("permission denied");
        match err {
            StoragePortError::ClearFailed(detail) => {
                assert_eq!(detail, "permission denied");
            }
            _ => panic!("Expected StoragePortError::ClearFailed"),
        }
    }

    #[test]
    fn test_notify_port_error_constructors() {
        let err = NotifyPortError::send("connection timeout");
        match err {
            NotifyPortError::SendFailed(detail) => {
                assert_eq!(detail, "connection timeout");
            }
        }
    }

    #[test]
    fn test_update_port_error_constructors() {
        let err = UpdatePortError::check("api offline");
        match err {
            UpdatePortError::CheckFailed(detail) => {
                assert_eq!(detail, "api offline");
            }
        }
    }
}
