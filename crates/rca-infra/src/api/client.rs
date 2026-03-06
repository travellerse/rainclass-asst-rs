use async_trait::async_trait;
use futures_core::Stream;
use reqwest::Url;
use std::pin::Pin;

use crate::api::{ApiError, WsEventDto};

#[derive(Debug, Clone)]
pub struct ApiClientConfig {
    pub base_url: Url,
    pub ws_url: Url,
    pub user_agent: String,
    pub connect_timeout_secs: u64,
    pub request_timeout_secs: u64,
    pub max_retries: u32,
}

#[derive(Debug, Clone)]
pub struct AuthContext {
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub user_id: u64,
}

#[derive(Debug, Clone)]
pub struct QrLoginSession {
    pub scene_id: String,
    pub token: String,
    pub qr_url: Url,
    pub expires_at_unix_ms: i64,
}

#[derive(Debug, Clone)]
pub enum QrLoginPollResult {
    Pending,
    Confirmed(AuthContext),
    Expired,
    Rejected,
}

pub type WsEventStream = Pin<Box<dyn Stream<Item = Result<WsEventDto, ApiError>> + Send>>;

#[async_trait]
pub trait RainClassroomWs: Send + Sync {
    async fn connect_lesson_stream(
        &self,
        auth: &AuthContext,
        lesson_id: u64,
    ) -> Result<WsEventStream, ApiError>;
}
