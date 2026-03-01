use async_trait::async_trait;

use crate::auth::{AuthError, AuthState};

#[derive(Debug, Clone)]
pub struct QrLoginBootstrap {
    pub scene_id: String,
    pub token: String,
    pub qr_svg: String,
}

#[derive(Debug, Clone)]
pub enum QrLoginProgress {
    Pending,
    Confirmed(AuthSession),
    Expired,
    Rejected,
}

#[derive(Debug, Clone)]
pub struct AuthSession {
    pub user_id: u64,
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub expires_at_unix_ms: Option<i64>,
}

#[async_trait]
pub trait AuthService: Send + Sync {
    async fn current_state(&self) -> AuthState;
    async fn begin_qr_login(&self) -> Result<QrLoginBootstrap, AuthError>;
    async fn poll_qr_login(&self, scene_id: &str) -> Result<QrLoginProgress, AuthError>;
    async fn restore_session(&self) -> Result<Option<AuthSession>, AuthError>;
    async fn refresh_if_needed(&self) -> Result<AuthSession, AuthError>;
    async fn logout(&self) -> Result<(), AuthError>;
}
