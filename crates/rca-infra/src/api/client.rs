use async_trait::async_trait;
use chrono::{DateTime, Utc};
use futures_core::Stream;
use reqwest::Url;
use std::pin::Pin;

use crate::api::{ApiError, CheckinDto, OnLessonDto, ProblemDto, WsEventDto};

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

#[derive(Debug, Clone)]
pub enum ApiAnswerPayload {
    Single { option_id: String },
    Multiple { option_ids: Vec<String> },
    FillBlank { text: String },
}

#[derive(Debug, Clone)]
pub struct ApiSubmitAnswerResult {
    pub accepted: bool,
    pub submitted_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct ApiCheckinResult {
    pub accepted: bool,
    pub submitted_at: DateTime<Utc>,
}

pub type WsEventStream = Pin<Box<dyn Stream<Item = Result<WsEventDto, ApiError>> + Send>>;

#[async_trait]
pub trait RainClassroomApi: Send + Sync {
    async fn get_on_lessons(&self, auth: &AuthContext) -> Result<Vec<OnLessonDto>, ApiError>;
    async fn get_lesson_problems(
        &self,
        auth: &AuthContext,
        lesson_id: u64,
    ) -> Result<Vec<ProblemDto>, ApiError>;
    async fn submit_answer(
        &self,
        auth: &AuthContext,
        lesson_id: u64,
        problem_id: u64,
        payload: ApiAnswerPayload,
    ) -> Result<ApiSubmitAnswerResult, ApiError>;
    async fn submit_checkin(
        &self,
        auth: &AuthContext,
        lesson_id: u64,
        checkin_id: u64,
    ) -> Result<ApiCheckinResult, ApiError>;
    async fn start_qr_login(&self) -> Result<QrLoginSession, ApiError>;
    async fn poll_qr_login(&self, session: &QrLoginSession) -> Result<QrLoginPollResult, ApiError>;
    async fn refresh_session(&self, refresh_token: &str) -> Result<AuthContext, ApiError>;
    async fn get_checkins(
        &self,
        auth: &AuthContext,
        lesson_id: u64,
    ) -> Result<Vec<CheckinDto>, ApiError>;
}

#[async_trait]
pub trait RainClassroomWs: Send + Sync {
    async fn connect_lesson_stream(
        &self,
        auth: &AuthContext,
        lesson_id: u64,
    ) -> Result<WsEventStream, ApiError>;
}
