mod download;
pub mod filename;
mod http;
mod parse;
mod pdf;
mod qr_login;
mod qr_state;
mod ws;

pub use filename::sanitize_filename_component;

use std::collections::HashSet;
use std::num::NonZeroU64;
use std::time::Duration;

use async_trait::async_trait;
use chrono::Utc;
use futures_util::StreamExt;
use reqwest::header::{AUTHORIZATION, CONTENT_TYPE, HeaderMap, HeaderValue};
use serde_json::{Value, json};
use tokio::sync::mpsc;

use rca_core::app::ports::{ApiPort, ApiPortError, LessonWsEvent};
use rca_core::auth::{AuthSession, QrLoginBootstrap, QrLoginProgress};
use rca_core::domain::{
    AnswerPayload, BlankAnswer, CheckinId, CourseId, Lesson, LessonId, LessonStatus, Problem,
    ProblemId, ProblemOption, ProblemType,
};

use crate::api::{ApiError, AuthContext, RainClassroomWs, WsEventDto};

#[derive(Debug, Clone, Copy)]
pub enum TenantHost {
    Rain,
    Hetang,
    Yangtze,
    YellowRiver,
}

impl TenantHost {
    pub fn as_host(self) -> &'static str {
        match self {
            TenantHost::Rain => "www.yuketang.cn",
            TenantHost::Hetang => "pro.yuketang.cn",
            TenantHost::Yangtze => "changjiang.yuketang.cn",
            TenantHost::YellowRiver => "huanghe.yuketang.cn",
        }
    }
}

#[derive(Debug, Clone)]
pub struct YktApiPortConfig {
    pub tenant: TenantHost,
    pub timeout_secs: u64,
}

#[derive(Clone)]
pub struct YktApiPort {
    client: reqwest::Client,
    host: String,
    user_agent: String,
    qr_state: qr_state::QrStateStore,
}

impl YktApiPort {
    pub fn new(config: YktApiPortConfig) -> Result<Self, ApiError> {
        let user_agent =
            "Mozilla/5.0 (Windows NT 10.0; Win64; x64; rv:97.0) Gecko/20100101 Firefox/97.0"
                .to_string();
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(config.timeout_secs.max(5)))
            .build()
            .map_err(ApiError::Http)?;

        Self::from_client_and_host(client, config.tenant.as_host().to_string(), user_agent)
    }

    fn from_client_and_host(
        client: reqwest::Client,
        host: String,
        user_agent: String,
    ) -> Result<Self, ApiError> {
        Ok(Self {
            client,
            host,
            user_agent,
            qr_state: qr_state::QrStateStore::new(),
        })
    }

    #[cfg(test)]
    fn new_for_test(base_url: &str) -> Result<Self, ApiError> {
        let user_agent =
            "Mozilla/5.0 (Windows NT 10.0; Win64; x64; rv:97.0) Gecko/20100101 Firefox/97.0"
                .to_string();
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(5))
            .danger_accept_invalid_certs(true)
            .build()
            .map_err(ApiError::Http)?;

        Self::from_client_and_host(client, base_url.to_string(), user_agent)
    }

    /// Build a PDF (bytes) from slide images.
    ///
    /// This is shared by the download pipeline and integration tests to ensure
    /// the generated PDFs remain parseable.
    pub fn build_presentation_pdf_bytes(
        width_px: f32,
        height_px: f32,
        slide_images: impl IntoIterator<Item = bytes::Bytes>,
    ) -> Result<Vec<u8>, ApiPortError> {
        pdf::build_presentation_pdf_bytes(width_px, height_px, slide_images)
    }

    fn session_headers(&self, session: &AuthSession) -> Result<HeaderMap, ApiError> {
        http::session_headers(&self.host, &self.user_agent, session)
    }

    fn to_non_zero(id: u64, field: &str) -> Result<NonZeroU64, ApiError> {
        NonZeroU64::new(id)
            .ok_or_else(|| ApiError::protocol("numeric id conversion", format!("{field} is zero")))
    }

    fn parse_api_ok(response: Value) -> Result<Value, ApiError> {
        let code = response.get("code").and_then(Value::as_i64).unwrap_or(-1);
        if code != 0 {
            let msg = response
                .get("msg")
                .and_then(Value::as_str)
                .unwrap_or("UNKNOWN_ERROR");
            return Err(ApiError::RemoteError {
                code,
                message: msg.to_string(),
            });
        }

        let data = response.get("data").cloned().unwrap_or(Value::Null);
        // Sometimes the user data is nested in data.user
        if let Some(user_data) = data.get("user") {
            Ok(user_data.clone())
        } else {
            Ok(data)
        }
    }

    fn extract_session_id(set_cookie_headers: &reqwest::header::HeaderMap) -> Option<String> {
        Self::extract_cookie_value(set_cookie_headers, "sessionid")
    }

    fn extract_csrf_token(set_cookie_headers: &reqwest::header::HeaderMap) -> Option<String> {
        Self::extract_cookie_value(set_cookie_headers, "csrftoken")
    }

    fn extract_cookie_value(
        set_cookie_headers: &reqwest::header::HeaderMap,
        cookie_name: &str,
    ) -> Option<String> {
        // Avoid allocating the prefix repeatedly for each header/part.
        // Use `split_once('=')` to compare the name without formatting a new String.
        for header in set_cookie_headers.get_all("set-cookie") {
            let raw = header.to_str().ok()?;
            for part in raw.split(';') {
                let trimmed = part.trim();
                if let Some((name, value)) = trimmed.split_once('=')
                    && name == cookie_name
                {
                    return Some(value.to_string());
                }
            }
        }
        None
    }

    fn build_page_view_headers(&self, session: &AuthSession) -> Result<HeaderMap, ApiPortError> {
        let csrf_token = session
            .csrf_token
            .as_deref()
            .filter(|value| !value.is_empty())
            .ok_or_else(|| ApiPortError::auth("missing csrftoken for page tracking"))?;

        let mut headers = self
            .session_headers(session)
            .map_err(ApiPortError::protocol)?;
        headers.insert(
            CONTENT_TYPE,
            HeaderValue::from_static("application/json;charset=utf-8"),
        );
        headers.insert("X-Client", HeaderValue::from_static("h5"));
        headers.insert("xtbz", HeaderValue::from_static("ykt"));
        // Use the session access token for Authorization header when reporting
        // tracking events; CSRF token is sent via cookie as required by the server.
        headers.insert(
            AUTHORIZATION,
            HeaderValue::from_str(&format!("Bearer {}", session.access_token))
                .map_err(|err| ApiPortError::protocol(format!("invalid auth header: {err}")))?,
        );

        let cookie = format!("sessionid={}; csrftoken={csrf_token}", session.access_token);
        headers.insert(
            reqwest::header::COOKIE,
            HeaderValue::from_str(&cookie)
                .map_err(|err| ApiPortError::protocol(format!("invalid track cookie: {err}")))?,
        );

        Ok(headers)
    }

    fn build_page_view_payload(
        &self,
        session: &AuthSession,
        lesson: &Lesson,
        slide_index: u64,
    ) -> Value {
        let ts_ms = Utc::now().timestamp_millis();
        self.build_page_view_payload_at(session, lesson, slide_index, ts_ms)
    }

    fn build_page_view_payload_at(
        &self,
        session: &AuthSession,
        lesson: &Lesson,
        slide_index: u64,
        ts_ms: i64,
    ) -> Value {
        let slide_page = slide_index.saturating_add(1);
        let lesson_id = lesson.lesson_id.0.get();
        let classroom_id = lesson.course_id.0.get();
        let page_url = format!(
            "https://{}/lesson/fullscreen/v3/{lesson_id}/ppt/{slide_page}",
            self.host
        );
        let index_url = format!("https://{}/v2/web/index", self.host);
        let original_id = session
            .original_id
            .clone()
            .unwrap_or_else(|| format!("user-{}", session.user_id));
        let distinct_id = session.user_id.to_string();

        json!({
            "uip": "",
            "data": {
                "platform": 2,
                "terminal_type": "web",
                "time": ts_ms,
                "language": "zh",
                "original_id": original_id,
                "distinct_id": distinct_id,
                "event": "page_view",
                "properties": {
                    "channel": "",
                    "user_agent": self.user_agent,
                    "url": page_url,
                    "classroom_id": classroom_id,
                    "page_name": lesson.course_name,
                    "host": self.host,
                    "referer": index_url,
                    "original_referrer": index_url
                }
            },
            "ts_ms": ts_ms
        })
    }

    #[cfg(test)]
    fn build_page_view_headers_for_base_url(
        &self,
        session: &AuthSession,
        base_url: &str,
    ) -> Result<HeaderMap, ApiPortError> {
        let csrf_token = session
            .csrf_token
            .as_deref()
            .filter(|value| !value.is_empty())
            .ok_or_else(|| ApiPortError::auth("missing csrftoken for page tracking"))?;

        let mut headers = HeaderMap::new();
        headers.insert(
            reqwest::header::COOKIE,
            HeaderValue::from_str(&format!(
                "sessionid={}; csrftoken={csrf_token}",
                session.access_token
            ))
            .map_err(|err| ApiPortError::protocol(format!("invalid track cookie: {err}")))?,
        );
        headers.insert(
            reqwest::header::USER_AGENT,
            HeaderValue::from_str(&self.user_agent)
                .map_err(|err| ApiPortError::protocol(format!("invalid user-agent: {err}")))?,
        );
        headers.insert(
            reqwest::header::ORIGIN,
            HeaderValue::from_str(base_url)
                .map_err(|err| ApiPortError::protocol(format!("invalid origin: {err}")))?,
        );
        headers.insert(
            reqwest::header::REFERER,
            HeaderValue::from_str(&format!("{base_url}/v2/web/index"))
                .map_err(|err| ApiPortError::protocol(format!("invalid referer: {err}")))?,
        );
        headers.insert(
            CONTENT_TYPE,
            HeaderValue::from_static("application/json;charset=utf-8"),
        );
        headers.insert("X-Client", HeaderValue::from_static("h5"));
        headers.insert("xtbz", HeaderValue::from_static("ykt"));
        // For test flows we also include an Authorization header with the access token
        headers.insert(
            AUTHORIZATION,
            HeaderValue::from_str(&format!("Bearer {}", session.access_token))
                .map_err(|err| ApiPortError::protocol(format!("invalid auth header: {err}")))?,
        );

        Ok(headers)
    }

    #[cfg(test)]
    fn build_page_view_payload_for_base_url_at(
        &self,
        session: &AuthSession,
        lesson: &Lesson,
        slide_index: u64,
        ts_ms: i64,
        base_url: &str,
    ) -> Value {
        let slide_page = slide_index.saturating_add(1);
        let lesson_id = lesson.lesson_id.0.get();
        let classroom_id = lesson.course_id.0.get();
        let page_url = format!("{base_url}/lesson/fullscreen/v3/{lesson_id}/ppt/{slide_page}");
        let index_url = format!("{base_url}/v2/web/index");
        let original_id = session
            .original_id
            .clone()
            .unwrap_or_else(|| format!("user-{}", session.user_id));
        let distinct_id = session.user_id.to_string();

        json!({
            "uip": "",
            "data": {
                "platform": 2,
                "terminal_type": "web",
                "time": ts_ms,
                "language": "zh",
                "original_id": original_id,
                "distinct_id": distinct_id,
                "event": "page_view",
                "properties": {
                    "channel": "",
                    "user_agent": self.user_agent,
                    "url": page_url,
                    "classroom_id": classroom_id,
                    "page_name": lesson.course_name,
                    "host": base_url,
                    "referer": index_url,
                    "original_referrer": index_url
                }
            },
            "ts_ms": ts_ms
        })
    }

    #[cfg(test)]
    async fn report_page_view_to_base_url(
        &self,
        base_url: &str,
        session: &AuthSession,
        lesson: &Lesson,
        slide_index: u64,
    ) -> Result<(), ApiPortError> {
        let headers = self.build_page_view_headers_for_base_url(session, base_url)?;
        let payload = self.build_page_view_payload_for_base_url_at(
            session,
            lesson,
            slide_index,
            Utc::now().timestamp_millis(),
            base_url,
        );

        let response = self
            .client
            .post(format!("{base_url}/video-log/log/track/"))
            .headers(headers)
            .json(&payload)
            .send()
            .await
            .map_err(|err| ApiPortError::request("track page_view", err))?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response
                .text()
                .await
                .unwrap_or_else(|_| "<unavailable>".to_string());
            return Err(ApiPortError::request(
                "track page_view",
                format!("unexpected status {status}: {body}"),
            ));
        }

        Ok(())
    }
}
fn map_problem_type(raw: &Value) -> ProblemType {
    parse::map_problem_type(raw)
}

fn parse_problem_options(problem: &Value) -> Vec<ProblemOption> {
    parse::parse_problem_options(problem)
}

fn parse_correct_answers(problem: &Value) -> Vec<String> {
    parse::parse_correct_answers(problem)
}

fn parse_blanks(problem: &Value) -> Vec<BlankAnswer> {
    parse::parse_blanks(problem)
}

fn parse_limit(problem: &Value) -> Option<i64> {
    parse::parse_limit(problem)
}

#[async_trait]
impl ApiPort for YktApiPort {
    async fn get_on_lessons(&self, session: &AuthSession) -> Result<Vec<Lesson>, ApiPortError> {
        let headers = self
            .session_headers(session)
            .map_err(ApiPortError::protocol)?;
        let url = format!("https://{}/api/v3/classroom/on-lesson", self.host);
        let value: Value = self
            .client
            .get(url)
            .headers(headers)
            .send()
            .await
            .map_err(|err| ApiPortError::request("on-lesson", err))?
            .json()
            .await
            .map_err(|err| ApiPortError::request("on-lesson decode", err))?;

        tracing::debug!("on-lesson raw response: {:?}", value);

        let data = Self::parse_api_ok(value).map_err(ApiPortError::protocol)?;
        let classrooms = data
            .get("onLessonClassrooms")
            .and_then(Value::as_array)
            .ok_or_else(|| ApiPortError::protocol("missing onLessonClassrooms"))?;

        let mut result = Vec::with_capacity(classrooms.len());
        for item in classrooms {
            let lesson_id_raw = item
                .get("lessonId")
                .and_then(|v| v.as_str())
                .and_then(|s| s.parse::<u64>().ok())
                .unwrap_or(0);
            let classroom_id_raw = item
                .get("classroomId")
                .and_then(|v| v.as_str())
                .and_then(|s| s.parse::<u64>().ok())
                .unwrap_or(0);
            if lesson_id_raw == 0 || classroom_id_raw == 0 {
                continue;
            }

            let lesson_id = LessonId(
                Self::to_non_zero(lesson_id_raw, "lessonId").map_err(ApiPortError::protocol)?,
            );
            let course_id = CourseId(
                Self::to_non_zero(classroom_id_raw, "classroomId")
                    .map_err(ApiPortError::protocol)?,
            );
            let course_name = item
                .get("courseName")
                .and_then(Value::as_str)
                .unwrap_or("未知课程")
                .to_string();

            result.push(Lesson {
                lesson_id,
                course_id,
                course_name,
                teacher_name: String::new(),
                started_at: None,
                ended_at: None,
                status: LessonStatus::Running,
            });
        }
        Ok(result)
    }

    async fn get_lesson_problems(
        &self,
        session: &AuthSession,
        lesson_id: LessonId,
    ) -> Result<Vec<Problem>, ApiPortError> {
        let headers = self
            .session_headers(session)
            .map_err(ApiPortError::protocol)?;
        let basic_info_url = format!("https://{}/api/v3/lesson/basic-info", self.host);
        let basic_info_value: Value = self
            .client
            .get(basic_info_url)
            .headers(headers.clone())
            .send()
            .await
            .map_err(|err| ApiPortError::request("lesson basic-info", err))?
            .json()
            .await
            .map_err(|err| ApiPortError::request("lesson basic-info decode", err))?;

        let basic_data = Self::parse_api_ok(basic_info_value).map_err(ApiPortError::protocol)?;
        let mut presentation_ids = HashSet::new();
        if let Some(presentation_id) = basic_data.get("presentation").and_then(Value::as_u64) {
            presentation_ids.insert(presentation_id);
        }
        if let Some(Value::Array(timeline)) = basic_data.get("timeline") {
            for item in timeline {
                let item_type = item.get("type").and_then(Value::as_str).unwrap_or_default();
                if item_type != "slide" {
                    continue;
                }
                if let Some(pres_id) = item.get("pres").and_then(Value::as_u64) {
                    presentation_ids.insert(pres_id);
                }
            }
        }

        let mut seen_problem_ids = HashSet::new();
        let mut problems = Vec::new();
        for presentation_id in presentation_ids {
            let fetch_url = format!(
                "https://{}/api/v3/lesson/presentation/fetch?presentation_id={}",
                self.host, presentation_id
            );
            let presentation_value: Value = self
                .client
                .get(fetch_url)
                .headers(headers.clone())
                .send()
                .await
                .map_err(|err| ApiPortError::request("presentation fetch", err))?
                .json()
                .await
                .map_err(|err| ApiPortError::request("presentation fetch decode", err))?;

            let presentation_data =
                Self::parse_api_ok(presentation_value).map_err(ApiPortError::protocol)?;
            let Some(Value::Array(slides)) = presentation_data.get("slides") else {
                continue;
            };

            for slide in slides {
                let Some(problem) = slide.get("problem") else {
                    continue;
                };

                let raw_problem_id = problem
                    .get("problemId")
                    .or_else(|| problem.get("sid"))
                    .and_then(Value::as_u64)
                    .unwrap_or(0);
                if raw_problem_id == 0 || !seen_problem_ids.insert(raw_problem_id) {
                    continue;
                }

                let title = problem
                    .get("title")
                    .or_else(|| problem.get("content"))
                    .or_else(|| problem.get("body"))
                    .and_then(Value::as_str)
                    .unwrap_or("未命名题目")
                    .to_string();
                let problem_type = map_problem_type(
                    problem
                        .get("problemType")
                        .or_else(|| problem.get("problem_type"))
                        .unwrap_or(&Value::Null),
                );
                let options = parse_problem_options(problem);

                // Extract correct answers from slide problem data
                let correct_answers = parse_correct_answers(problem);
                let blanks = parse_blanks(problem);
                let limit_secs = parse_limit(problem);

                let problem_id = ProblemId(
                    Self::to_non_zero(raw_problem_id, "problemId")
                        .map_err(ApiPortError::protocol)?,
                );

                problems.push(Problem {
                    lesson_id,
                    problem_id,
                    problem_type,
                    title,
                    options,
                    correct_answers,
                    blanks,
                    limit_secs,
                    published_at: Utc::now(),
                    deadline_at: None,
                });
            }
        }

        Ok(problems)
    }

    async fn submit_answer(
        &self,
        session: &AuthSession,
        lesson_id: LessonId,
        problem_id: ProblemId,
        payload: AnswerPayload,
    ) -> Result<(), ApiPortError> {
        let headers = self
            .session_headers(session)
            .map_err(ApiPortError::protocol)?;
        let (problem_type, result_value) = match payload {
            AnswerPayload::Single { option_id } => (1_i64, json!([option_id])),
            AnswerPayload::Multiple { option_ids } => (2_i64, json!(option_ids)),
            AnswerPayload::FillBlank { text } => (3_i64, json!([text])),
        };

        let body = json!({
            "lessonId": lesson_id.0.get(),
            "problemId": problem_id.0.get(),
            "problemType": problem_type,
            "dt": Utc::now().timestamp(),
            "result": result_value,
        });

        let url = format!("https://{}/api/v3/lesson/problem/answer", self.host);
        let value: Value = self
            .client
            .post(url)
            .headers(headers)
            .json(&body)
            .send()
            .await
            .map_err(|err| ApiPortError::request("submit answer", err))?
            .json()
            .await
            .map_err(|err| ApiPortError::request("submit answer decode", err))?;

        let _ = Self::parse_api_ok(value).map_err(ApiPortError::protocol)?;
        Ok(())
    }

    async fn submit_checkin(
        &self,
        session: &AuthSession,
        lesson_id: LessonId,
        _checkin_id: CheckinId,
    ) -> Result<(), ApiPortError> {
        let headers = self
            .session_headers(session)
            .map_err(ApiPortError::protocol)?;
        let body = json!({
            "source": 5,
            "lessonId": lesson_id.0.get(),
        });

        let url = format!("https://{}/api/v3/lesson/checkin", self.host);
        let value: Value = self
            .client
            .post(url)
            .headers(headers)
            .json(&body)
            .send()
            .await
            .map_err(|err| ApiPortError::request("submit checkin", err))?
            .json()
            .await
            .map_err(|err| ApiPortError::request("submit checkin decode", err))?;

        let _ = Self::parse_api_ok(value).map_err(ApiPortError::protocol)?;
        Ok(())
    }

    async fn send_danmu(
        &self,
        session: &AuthSession,
        lesson_id: LessonId,
        content: &str,
    ) -> Result<(), ApiPortError> {
        let headers = self
            .session_headers(session)
            .map_err(ApiPortError::protocol)?;
        let body = json!({
            "lessonId": lesson_id.0.get(),
            "coursewormId": lesson_id.0.get(), // Rain Classroom requires this field conceptually identical to lessonId
            "message": content,
        });

        let url = format!("https://{}/api/v3/lesson/danmu/send", self.host);
        let value: Value = self
            .client
            .post(url)
            .headers(headers)
            .json(&body)
            .send()
            .await
            .map_err(|err| ApiPortError::request("send danmu", err))?
            .json()
            .await
            .map_err(|err| ApiPortError::request("send danmu decode", err))?;

        let _ = Self::parse_api_ok(value).map_err(ApiPortError::protocol)?;
        Ok(())
    }

    async fn start_qr_login(&self) -> Result<QrLoginBootstrap, ApiPortError> {
        let scene_id = format!("scene-{}", Utc::now().timestamp_millis());
        self.qr_state
            .set(&scene_id, qr_state::QrSceneState::Pending);

        tracing::info!(
            target: "rca_infra.api",
            host = %self.host,
            scene_id = %scene_id,
            "开始二维码登录"
        );

        qr_login::start_qr_login(self, self.qr_state.clone(), scene_id).await
    }

    async fn poll_qr_login(&self, scene_id: &str) -> Result<QrLoginProgress, ApiPortError> {
        self.qr_state.poll(scene_id)
    }

    async fn wait_qr_login(
        &self,
        scene_id: &str,
        timeout_secs: u64,
    ) -> Result<QrLoginProgress, ApiPortError> {
        self.qr_state.wait(scene_id, timeout_secs).await
    }

    async fn download_presentation(
        &self,
        session: &AuthSession,
        presentation_id: u64,
        lesson_id: Option<u64>,
        save_dir: &std::path::Path,
    ) -> Result<std::path::PathBuf, ApiPortError> {
        download::download_presentation(self, session, presentation_id, lesson_id, save_dir).await
    }

    async fn refresh_session(&self, refresh_token: &str) -> Result<AuthSession, ApiPortError> {
        let session = AuthSession {
            user_id: 0,
            access_token: refresh_token.to_string(),
            refresh_token: None,
            expires_at_unix_ms: None,
            csrf_token: None,
            original_id: None,
        };
        let headers = self
            .session_headers(&session)
            .map_err(ApiPortError::protocol)?;
        let url = format!("https://{}/api/v3/user/basic-info", self.host);
        let value: Value = self
            .client
            .get(url)
            .headers(headers)
            .send()
            .await
            .map_err(|err| ApiPortError::request("refresh basic-info", err))?
            .json()
            .await
            .map_err(|err| ApiPortError::request("refresh basic-info decode", err))?;

        let data = Self::parse_api_ok(value.clone()).map_err(|e| {
            tracing::warn!("refresh basic-info parse error: {e}, payload: {value}");
            ApiPortError::protocol(e)
        })?;

        let user_id = data
            .get("userId")
            .and_then(Value::as_u64)
            .or_else(|| data.get("id").and_then(Value::as_u64))
            .or_else(|| data.get("user_id").and_then(Value::as_u64))
            // Handle the case where the id is parsed as a string containing numbers
            .or_else(|| {
                data.get("id")
                    .and_then(Value::as_str)
                    .and_then(|s| s.parse::<u64>().ok())
            })
            .unwrap_or(0);

        if user_id == 0 {
            tracing::warn!(
                "Failed to extract user ID from basic-info response: {}",
                data
            );
            return Err(ApiPortError::protocol("refresh basic-info missing user id"));
        }

        Ok(AuthSession {
            user_id,
            access_token: refresh_token.to_string(),
            refresh_token: None,
            expires_at_unix_ms: None,
            csrf_token: None,
            original_id: None,
        })
    }

    async fn report_page_view(
        &self,
        session: &AuthSession,
        lesson: &Lesson,
        slide_index: u64,
    ) -> Result<(), ApiPortError> {
        let headers = self.build_page_view_headers(session)?;
        let payload = self.build_page_view_payload(session, lesson, slide_index);

        let response = self
            .client
            .post(format!("https://{}/video-log/log/track/", self.host))
            .headers(headers)
            .json(&payload)
            .send()
            .await
            .map_err(|err| ApiPortError::request("track page_view", err))?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response
                .text()
                .await
                .unwrap_or_else(|_| "<unavailable>".to_string());
            return Err(ApiPortError::request(
                "track page_view",
                format!("unexpected status {status}: {body}"),
            ));
        }

        Ok(())
    }

    async fn connect_lesson_stream(
        &self,
        session: &AuthSession,
        lesson_id: LessonId,
    ) -> Result<mpsc::Receiver<LessonWsEvent>, ApiPortError> {
        let mut ws_stream = RainClassroomWs::connect_lesson_stream(
            self,
            &AuthContext {
                access_token: session.access_token.clone(),
                refresh_token: session.refresh_token.clone(),
                user_id: session.user_id,
            },
            lesson_id.0.get(),
        )
        .await
        .map_err(|err| ApiPortError::request("connect lesson ws", err))?;

        let (tx, rx) = mpsc::channel(128);
        let lesson_id_copy = lesson_id;
        tokio::spawn(async move {
            while let Some(item) = ws_stream.next().await {
                let mapped = match item {
                    Ok(WsEventDto::ProblemPublished(problem)) => {
                        let problem_id = match NonZeroU64::new(problem.problem_id) {
                            Some(id) => ProblemId(id),
                            None => {
                                let _ = tx
                                    .send(LessonWsEvent::Warning {
                                        message: "ws problem id is zero".to_string(),
                                    })
                                    .await;
                                continue;
                            }
                        };
                        let problem_type = map_problem_type(&Value::String(problem.problem_type));
                        let options = problem
                            .options
                            .into_iter()
                            .map(|(option_id, text)| ProblemOption { option_id, text })
                            .collect::<Vec<_>>();
                        let blanks = problem
                            .blanks
                            .into_iter()
                            .map(|accepted_values| BlankAnswer { accepted_values })
                            .collect();
                        LessonWsEvent::ProblemPublished {
                            problem: Problem {
                                lesson_id: lesson_id_copy,
                                problem_id,
                                problem_type,
                                title: problem.title,
                                options,
                                correct_answers: problem.correct_answers,
                                blanks,
                                limit_secs: problem.limit_secs,
                                published_at: problem.published_at,
                                deadline_at: problem.deadline_at,
                            },
                        }
                    }
                    Ok(WsEventDto::CheckinOpened(checkin)) => {
                        let Some(checkin_id) = NonZeroU64::new(checkin.checkin_id).map(CheckinId)
                        else {
                            let _ = tx
                                .send(LessonWsEvent::Warning {
                                    message: "ws checkin id is zero".to_string(),
                                })
                                .await;
                            continue;
                        };
                        LessonWsEvent::CheckinOpened { checkin_id }
                    }
                    Ok(WsEventDto::DanmuPublished(danmu)) => LessonWsEvent::DanmuPublished {
                        user_name: danmu.user_name,
                        content: danmu.content,
                    },
                    Ok(WsEventDto::CallPaused(call)) => LessonWsEvent::CallPaused {
                        target_name: call.target_name,
                    },
                    Ok(WsEventDto::PresentationUpdated(pres)) => {
                        LessonWsEvent::PresentationUpdated {
                            presentation_id: pres.presentation_id,
                        }
                    }
                    Ok(WsEventDto::SlideNavigated(nav)) => LessonWsEvent::SlideNavigated {
                        presentation_id: nav.presentation_id,
                        slide_id: nav.slide_id,
                        slide_index: nav.slide_index,
                    },
                    Ok(WsEventDto::ProblemUnlocked(prob)) => {
                        if let Some(problem_id) = NonZeroU64::new(prob.problem_id).map(ProblemId) {
                            LessonWsEvent::ProblemUnlocked { problem_id }
                        } else {
                            LessonWsEvent::Warning {
                                message: "ws unlock problem id is zero".to_string(),
                            }
                        }
                    }
                    Ok(WsEventDto::LessonEnded { .. }) => LessonWsEvent::LessonEnded,
                    Ok(WsEventDto::Unknown { raw_type, .. }) => LessonWsEvent::Unknown { raw_type },
                    Err(err) => LessonWsEvent::Warning {
                        message: format!("ws stream error: {err}"),
                    },
                };

                if tx.send(mapped).await.is_err() {
                    break;
                }
            }
        });

        Ok(rx)
    }
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use rca_core::domain::ProblemType;

    use super::*;

    // Use a non-sensitive test user id to avoid leaking real identifiers in test code
    const TEST_USER_ID: u64 = 12345;
    // Use non-sensitive test ids for lesson and classroom
    const TEST_LESSON_ID: u64 = 16484;
    const TEST_CLASSROOM_ID: u64 = 31766;

    // ── TenantHost ─────────────────────────────────────────────

    #[test]
    fn tenant_host_rain() {
        assert_eq!(TenantHost::Rain.as_host(), "www.yuketang.cn");
    }

    #[test]
    fn tenant_host_hetang() {
        assert_eq!(TenantHost::Hetang.as_host(), "pro.yuketang.cn");
    }

    #[test]
    fn tenant_host_yangtze() {
        assert_eq!(TenantHost::Yangtze.as_host(), "changjiang.yuketang.cn");
    }

    #[test]
    fn tenant_host_yellowriver() {
        assert_eq!(TenantHost::YellowRiver.as_host(), "huanghe.yuketang.cn");
    }

    // ── map_problem_type ───────────────────────────────────────

    #[test]
    fn map_problem_type_numeric_codes() {
        assert_eq!(map_problem_type(&json!(1)), ProblemType::SingleChoice);
        assert_eq!(map_problem_type(&json!(2)), ProblemType::MultipleChoice);
        assert_eq!(map_problem_type(&json!(3)), ProblemType::FillBlank);
        assert_eq!(map_problem_type(&json!(99)), ProblemType::Unknown);
    }

    #[test]
    fn map_problem_type_string_patterns() {
        assert_eq!(
            map_problem_type(&json!("single")),
            ProblemType::SingleChoice
        );
        assert_eq!(
            map_problem_type(&json!("choice")),
            ProblemType::SingleChoice
        );
        assert_eq!(
            map_problem_type(&json!("MULTIPLE")),
            ProblemType::MultipleChoice
        );
        assert_eq!(
            map_problem_type(&json!("fill_blank")),
            ProblemType::FillBlank
        );
        assert_eq!(map_problem_type(&json!("fill")), ProblemType::FillBlank);
        assert_eq!(map_problem_type(&json!("blank")), ProblemType::FillBlank);
        assert_eq!(map_problem_type(&json!("essay")), ProblemType::Unknown);
        assert_eq!(map_problem_type(&json!("")), ProblemType::Unknown);
    }

    #[test]
    fn map_problem_type_null() {
        assert_eq!(map_problem_type(&Value::Null), ProblemType::Unknown);
    }

    // ── parse_problem_options ──────────────────────────────────

    #[test]
    fn parse_options_from_object_array() {
        let problem = json!({
            "options": [
                {"optionId": "A", "text": "Alpha"},
                {"optionId": "B", "text": "Beta"},
            ]
        });
        let opts = parse_problem_options(&problem);
        assert_eq!(opts.len(), 2);
        assert_eq!(opts[0].option_id, "A");
        assert_eq!(opts[0].text, "Alpha");
        assert_eq!(opts[1].option_id, "B");
    }

    #[test]
    fn parse_options_from_string_array() {
        let problem = json!({
            "options": ["Yes", "No"]
        });
        let opts = parse_problem_options(&problem);
        assert_eq!(opts.len(), 2);
        assert_eq!(opts[0].option_id, "0");
        assert_eq!(opts[0].text, "Yes");
        assert_eq!(opts[1].option_id, "1");
    }

    #[test]
    fn parse_options_fallback_to_choices_key() {
        let problem = json!({
            "choices": [{"id": "X", "content": "Choice X"}]
        });
        let opts = parse_problem_options(&problem);
        assert_eq!(opts.len(), 1);
        assert_eq!(opts[0].option_id, "X");
        assert_eq!(opts[0].text, "Choice X");
    }

    #[test]
    fn parse_options_fallback_to_answers_when_empty() {
        let problem = json!({
            "answers": ["opt1", "opt2"]
        });
        let opts = parse_problem_options(&problem);
        assert_eq!(opts.len(), 2);
        assert_eq!(opts[0].option_id, "opt1");
    }

    #[test]
    fn parse_options_empty_when_no_keys() {
        let problem = json!({"title": "test"});
        let opts = parse_problem_options(&problem);
        assert!(opts.is_empty());
    }

    #[test]
    fn parse_options_object_fallback_keys() {
        // Uses "key" for option_id, "label" for text
        let problem = json!({
            "options": [{"key": "K1", "label": "Label1"}]
        });
        let opts = parse_problem_options(&problem);
        assert_eq!(opts[0].option_id, "K1");
        assert_eq!(opts[0].text, "Label1");
    }

    // ── parse_correct_answers ──────────────────────────────────

    #[test]
    fn parse_correct_answers_mixed_types() {
        let problem = json!({
            "answers": ["A", 42, true]
        });
        let answers = parse_correct_answers(&problem);
        assert_eq!(answers, vec!["A", "42", "true"]);
    }

    #[test]
    fn parse_correct_answers_empty() {
        let problem = json!({"title": "no answers"});
        let answers = parse_correct_answers(&problem);
        assert!(answers.is_empty());
    }

    #[test]
    fn parse_correct_answers_ignores_non_primitives() {
        let problem = json!({
            "answers": [{"complex": true}, "valid"]
        });
        let answers = parse_correct_answers(&problem);
        assert_eq!(answers, vec!["valid"]);
    }

    // ── parse_blanks ───────────────────────────────────────────

    #[test]
    fn parse_blanks_multiple() {
        let problem = json!({
            "blanks": [
                {"answers": ["hello", "hi"]},
                {"answers": [42]},
            ]
        });
        let blanks = parse_blanks(&problem);
        assert_eq!(blanks.len(), 2);
        assert_eq!(blanks[0].accepted_values, vec!["hello", "hi"]);
        assert_eq!(blanks[1].accepted_values, vec!["42"]);
    }

    #[test]
    fn parse_blanks_empty() {
        let problem = json!({"title": "no blanks"});
        let blanks = parse_blanks(&problem);
        assert!(blanks.is_empty());
    }

    #[test]
    fn parse_blanks_blank_without_answers() {
        let problem = json!({
            "blanks": [{"some_field": "value"}]
        });
        let blanks = parse_blanks(&problem);
        assert_eq!(blanks.len(), 1);
        assert!(blanks[0].accepted_values.is_empty());
    }

    // ── parse_limit ────────────────────────────────────────────

    #[test]
    fn parse_limit_normal() {
        let problem = json!({"limit": 60});
        assert_eq!(parse_limit(&problem), Some(60));
    }

    #[test]
    fn parse_limit_unlimited() {
        let problem = json!({"limit": -1});
        assert_eq!(parse_limit(&problem), None);
    }

    #[test]
    fn parse_limit_missing() {
        let problem = json!({"title": "test"});
        assert_eq!(parse_limit(&problem), None);
    }

    #[test]
    fn parse_limit_zero() {
        let problem = json!({"limit": 0});
        assert_eq!(parse_limit(&problem), Some(0));
    }

    // ── YktApiPort::parse_api_ok ───────────────────────────────

    #[test]
    fn parse_api_ok_success() {
        let resp = json!({"code": 0, "data": {"key": "value"}});
        let result = YktApiPort::parse_api_ok(resp).unwrap();
        assert_eq!(result, json!({"key": "value"}));
    }

    #[test]
    fn parse_api_ok_error() {
        let resp = json!({"code": 1001, "msg": "bad request"});
        let result = YktApiPort::parse_api_ok(resp);
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(err_msg.contains("1001"));
        assert!(err_msg.contains("bad request"));
    }

    #[test]
    fn parse_api_ok_missing_data() {
        let resp = json!({"code": 0});
        let result = YktApiPort::parse_api_ok(resp).unwrap();
        assert_eq!(result, Value::Null);
    }

    // ── YktApiPort::extract_session_id ─────────────────────────

    #[test]
    fn extract_session_id_found() {
        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert(
            "set-cookie",
            "sessionid=abc123; Path=/; HttpOnly".parse().unwrap(),
        );
        let result = YktApiPort::extract_session_id(&headers);
        assert_eq!(result, Some("abc123".to_string()));
    }

    #[test]
    fn extract_session_id_not_found() {
        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert("set-cookie", "other=value; Path=/".parse().unwrap());
        let result = YktApiPort::extract_session_id(&headers);
        assert!(result.is_none());
    }

    #[test]
    fn extract_session_id_empty_headers() {
        let headers = reqwest::header::HeaderMap::new();
        let result = YktApiPort::extract_session_id(&headers);
        assert!(result.is_none());
    }

    #[test]
    fn extract_csrf_token_found() {
        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert(
            "set-cookie",
            "csrftoken=csrf123; Path=/; Secure".parse().unwrap(),
        );
        let result = YktApiPort::extract_csrf_token(&headers);
        assert_eq!(result, Some("csrf123".to_string()));
    }

    #[test]
    fn build_page_view_headers_contains_required_fields() {
        let api = YktApiPort::new(YktApiPortConfig {
            tenant: TenantHost::Hetang,
            timeout_secs: 5,
        })
        .unwrap();
        let session = AuthSession {
            user_id: 42,
            access_token: "session-token".to_string(),
            refresh_token: None,
            expires_at_unix_ms: None,
            csrf_token: Some("csrf-token".to_string()),
            original_id: Some("orig-42".to_string()),
        };

        let headers = api.build_page_view_headers(&session).unwrap();

        assert_eq!(headers.get("X-Client").unwrap(), "h5");
        assert_eq!(headers.get("xtbz").unwrap(), "ykt");
        assert_eq!(
            headers.get(CONTENT_TYPE).unwrap(),
            "application/json;charset=utf-8"
        );
        assert_eq!(headers.get(AUTHORIZATION).unwrap(), "Bearer session-token");
        assert_eq!(
            headers.get(reqwest::header::COOKIE).unwrap(),
            "sessionid=session-token; csrftoken=csrf-token"
        );
    }

    #[test]
    fn build_page_view_payload_contains_expected_tracking_fields() {
        let api = YktApiPort::new(YktApiPortConfig {
            tenant: TenantHost::Hetang,
            timeout_secs: 5,
        })
        .unwrap();
        let session = AuthSession {
            user_id: TEST_USER_ID,
            access_token: "session-token".to_string(),
            refresh_token: None,
            expires_at_unix_ms: None,
            csrf_token: Some("csrf-token".to_string()),
            original_id: Some("device-orig".to_string()),
        };
        let lesson = Lesson {
            lesson_id: LessonId(NonZeroU64::new(TEST_LESSON_ID).unwrap()),
            course_id: CourseId(NonZeroU64::new(TEST_CLASSROOM_ID).unwrap()),
            course_name: "人工智能产业导引".to_string(),
            teacher_name: "teacher".to_string(),
            started_at: None,
            ended_at: None,
            status: LessonStatus::Running,
        };

        let payload = api.build_page_view_payload_at(&session, &lesson, 11, 1_774_352_483_886);

        assert_eq!(payload["ts_ms"], 1_774_352_483_886_i64);
        assert_eq!(payload["data"]["time"], 1_774_352_483_886_i64);
        assert_eq!(payload["data"]["event"], "page_view");
        assert_eq!(payload["data"]["platform"], 2);
        assert_eq!(payload["data"]["terminal_type"], "web");
        assert_eq!(payload["data"]["original_id"], "device-orig");
        assert_eq!(payload["data"]["distinct_id"], TEST_USER_ID.to_string());
        assert_eq!(
            payload["data"]["properties"]["classroom_id"],
            TEST_CLASSROOM_ID
        );
        assert_eq!(
            payload["data"]["properties"]["page_name"],
            "人工智能产业导引"
        );
        assert_eq!(
            payload["data"]["properties"]["url"].as_str().unwrap(),
            &format!(
                "https://pro.yuketang.cn/lesson/fullscreen/v3/{}/ppt/12",
                TEST_LESSON_ID
            )
        );
        assert_eq!(
            payload["data"]["properties"]["user_agent"]
                .as_str()
                .unwrap(),
            api.user_agent.as_str()
        );
    }

    #[test]
    fn build_page_view_payload_falls_back_to_synthetic_original_id() {
        let api = YktApiPort::new(YktApiPortConfig {
            tenant: TenantHost::Hetang,
            timeout_secs: 5,
        })
        .unwrap();
        let session = AuthSession {
            user_id: 99,
            access_token: "session-token".to_string(),
            refresh_token: None,
            expires_at_unix_ms: None,
            csrf_token: Some("csrf-token".to_string()),
            original_id: None,
        };
        let lesson = Lesson {
            lesson_id: LessonId(NonZeroU64::new(88).unwrap()),
            course_id: CourseId(NonZeroU64::new(77).unwrap()),
            course_name: "course".to_string(),
            teacher_name: "teacher".to_string(),
            started_at: None,
            ended_at: None,
            status: LessonStatus::Running,
        };

        let payload = api.build_page_view_payload_at(&session, &lesson, 0, 123456789);

        assert_eq!(payload["data"]["original_id"], "user-99");
        assert_eq!(
            payload["data"]["properties"]["url"],
            "https://pro.yuketang.cn/lesson/fullscreen/v3/88/ppt/1"
        );
    }

    #[tokio::test]
    async fn report_page_view_sends_expected_request_to_server() {
        let mut server = mockito::Server::new_async().await;
        let api = YktApiPort::new_for_test(&server.url()).unwrap();
        let session = AuthSession {
            user_id: TEST_USER_ID,
            access_token: "session-token".to_string(),
            refresh_token: None,
            expires_at_unix_ms: None,
            csrf_token: Some("csrf-token".to_string()),
            original_id: Some("device-orig".to_string()),
        };
        let lesson = Lesson {
            lesson_id: LessonId(NonZeroU64::new(TEST_LESSON_ID).unwrap()),
            course_id: CourseId(NonZeroU64::new(TEST_CLASSROOM_ID).unwrap()),
            course_name: "人工智能产业导引".to_string(),
            teacher_name: "teacher".to_string(),
            started_at: None,
            ended_at: None,
            status: LessonStatus::Running,
        };

        let mock = server
            .mock("POST", "/video-log/log/track/")
            .match_header("x-client", "h5")
            .match_header("xtbz", "ykt")
            .match_header("content-type", mockito::Matcher::Regex("^application/json".to_string()))
            .match_header("authorization", "Bearer session-token")
            .match_header("cookie", "sessionid=session-token; csrftoken=csrf-token")
            .match_body(mockito::Matcher::PartialJson(json!({
                "data": {
                    "platform": 2,
                    "terminal_type": "web",
                    "language": "zh",
                    "original_id": "device-orig",
                    "distinct_id": TEST_USER_ID.to_string(),
                    "event": "page_view",
                    "properties": {
                        "url": format!("{}/lesson/fullscreen/v3/{}/ppt/12", server.url(), TEST_LESSON_ID),
                        "classroom_id": TEST_CLASSROOM_ID,
                        "page_name": "人工智能产业导引",
                        "host": server.url(),
                        "referer": format!("{}/v2/web/index", server.url()),
                        "original_referrer": format!("{}/v2/web/index", server.url())
                    }
                }
            })))
            .with_status(200)
            .create_async()
            .await;

        api.report_page_view_to_base_url(&server.url(), &session, &lesson, 11)
            .await
            .unwrap();

        mock.assert_async().await;
    }

    #[test]
    fn test_map_problem_type() {
        assert_eq!(map_problem_type(&json!(1)), ProblemType::SingleChoice);
        assert_eq!(map_problem_type(&json!(2)), ProblemType::MultipleChoice);
        assert_eq!(map_problem_type(&json!(3)), ProblemType::FillBlank);
        assert_eq!(map_problem_type(&json!(99)), ProblemType::Unknown);
        assert_eq!(
            map_problem_type(&json!("Single")),
            ProblemType::SingleChoice
        );
        assert_eq!(
            map_problem_type(&json!("multiple choice")),
            ProblemType::MultipleChoice
        );
        assert_eq!(
            map_problem_type(&json!("fill in the blank")),
            ProblemType::FillBlank
        );
    }

    #[test]
    fn test_parse_problem_options() {
        let problem = json!({
            "options": [
                {"optionId": "A", "text": "Option A"},
                {"option_id": "B", "content": "Option B"}
            ]
        });
        let options = parse_problem_options(&problem);
        assert_eq!(options.len(), 2);
        assert_eq!(options[0].option_id, "A");
        assert_eq!(options[1].option_id, "B");

        let problem_v2 = json!({
            "choiceList": ["C", "D"]
        });
        let options_v2 = parse_problem_options(&problem_v2);
        assert_eq!(options_v2.len(), 2);
        assert_eq!(options_v2[0].option_id, "0");
        assert_eq!(options_v2[0].text, "C");
    }

    #[test]
    fn test_parse_correct_answers() {
        let problem = json!({
            "answers": ["A", 2, true]
        });
        let answers = parse_correct_answers(&problem);
        assert_eq!(answers, vec!["A", "2", "true"]);
    }

    #[test]
    fn test_parse_blanks() {
        let problem = json!({
            "blanks": [
                {"answers": ["one", 1]},
                {"answers": ["two"]}
            ]
        });
        let blanks = parse_blanks(&problem);
        assert_eq!(blanks.len(), 2);
        assert_eq!(blanks[0].accepted_values, vec!["one", "1"]);
    }

    #[test]
    fn test_parse_limit() {
        assert_eq!(parse_limit(&json!({"limit": 60})), Some(60));
        assert_eq!(parse_limit(&json!({"limit": -1})), None);
        assert_eq!(parse_limit(&json!({})), None);
    }
}
