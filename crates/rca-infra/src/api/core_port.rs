use std::collections::HashMap;
use std::collections::HashSet;
use std::num::NonZeroU64;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use chrono::Utc;
use futures_util::stream;
use futures_util::{SinkExt, StreamExt};
use reqwest::header::{AUTHORIZATION, COOKIE, HeaderMap, HeaderValue, USER_AGENT};
use serde_json::{Value, json};
use tokio::sync::{Notify, mpsc, oneshot};
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;

use rca_core::app::ports::{ApiPort, ApiPortError, LessonWsEvent};
use rca_core::auth::{AuthSession, QrLoginBootstrap, QrLoginProgress};
use rca_core::domain::{
    AnswerPayload, BlankAnswer, CheckinId, CourseId, Lesson, LessonId, LessonStatus, Problem,
    ProblemId, ProblemOption, ProblemType,
};

use crate::api::{ApiError, AuthContext, RainClassroomWs, WsEventDto, WsEventStream};

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
    qr_states: Arc<Mutex<HashMap<String, QrSceneState>>>,
    qr_state_notify: Arc<Notify>,
}

#[derive(Debug, Clone)]
enum QrSceneState {
    Pending,
    Confirmed(AuthSession),
    Expired,
    Rejected,
}

impl YktApiPort {
    pub fn new(config: YktApiPortConfig) -> Result<Self, ApiError> {
        let user_agent =
            "Mozilla/5.0 (X11; Linux x86_64) RainClassroomAssistant-Rust/0.1".to_string();
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(config.timeout_secs.max(5)))
            .build()
            .map_err(ApiError::Http)?;

        Ok(Self {
            client,
            host: config.tenant.as_host().to_string(),
            user_agent,
            qr_states: Arc::new(Mutex::new(HashMap::new())),
            qr_state_notify: Arc::new(Notify::new()),
        })
    }

    fn session_headers(&self, session: &AuthSession) -> Result<HeaderMap, ApiError> {
        let mut headers = HeaderMap::new();
        let cookie = format!("sessionid={}", session.access_token);
        headers.insert(
            COOKIE,
            HeaderValue::from_str(&cookie)
                .map_err(|err| ApiError::invalid_header("cookie", err))?,
        );
        headers.insert(
            USER_AGENT,
            HeaderValue::from_str(&self.user_agent)
                .map_err(|err| ApiError::invalid_header("user-agent", err))?,
        );
        Ok(headers)
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
        Ok(response.get("data").cloned().unwrap_or(Value::Null))
    }

    fn extract_session_id(set_cookie_headers: &reqwest::header::HeaderMap) -> Option<String> {
        for header in set_cookie_headers.get_all("set-cookie") {
            let raw = header.to_str().ok()?;
            for part in raw.split(';') {
                let trimmed = part.trim();
                if let Some(value) = trimmed.strip_prefix("sessionid=") {
                    return Some(value.to_string());
                }
            }
        }
        None
    }

    fn update_qr_state(&self, scene_id: &str, state: QrSceneState) {
        let mut states = self.qr_states.lock().expect("qr state poisoned");
        states.insert(scene_id.to_string(), state);
        self.qr_state_notify.notify_waiters();
    }

    fn update_qr_state_shared(
        states: &Arc<Mutex<HashMap<String, QrSceneState>>>,
        notifier: &Arc<Notify>,
        scene_id: &str,
        state: QrSceneState,
    ) {
        let mut guard = states.lock().expect("qr state poisoned");
        guard.insert(scene_id.to_string(), state);
        notifier.notify_waiters();
    }

    fn map_ws_problem(problem_value: &Value, lesson_id: u64) -> Option<WsEventDto> {
        let problem_id = problem_value
            .get("sid")
            .or_else(|| problem_value.get("problemId"))
            .or_else(|| problem_value.get("problemid"))
            .and_then(Value::as_u64)?;
        let problem_type = problem_value
            .get("problemType")
            .or_else(|| problem_value.get("problem_type"))
            .map(|v| {
                if let Some(code) = v.as_i64() {
                    match code {
                        1 => "single",
                        2 => "multiple",
                        3 => "fill_blank",
                        _ => "unknown",
                    }
                } else {
                    v.as_str().unwrap_or("unknown")
                }
            })
            .unwrap_or("unknown")
            .to_string();
        let title = problem_value
            .get("title")
            .or_else(|| problem_value.get("content"))
            .or_else(|| problem_value.get("body"))
            .and_then(Value::as_str)
            .unwrap_or("WS 题目")
            .to_string();
        let limit = problem_value
            .get("limit")
            .and_then(Value::as_i64)
            .and_then(|v| if v == -1 { None } else { Some(v) });

        Some(WsEventDto::ProblemPublished(crate::api::ProblemDto {
            lesson_id,
            problem_id,
            problem_type,
            title,
            options: Vec::new(),
            correct_answers: Vec::new(),
            blanks: Vec::new(),
            limit_secs: limit,
            published_at: Utc::now(),
            deadline_at: None,
        }))
    }

    async fn prepare_lesson_ws_auth(
        &self,
        auth: &AuthContext,
        lesson_id: u64,
    ) -> Result<(u64, String, String), ApiError> {
        let session = AuthSession {
            user_id: auth.user_id,
            access_token: auth.access_token.clone(),
            refresh_token: auth.refresh_token.clone(),
            expires_at_unix_ms: None,
        };
        let headers = self
            .session_headers(&session)
            .map_err(|err| ApiError::protocol("build session headers", err))?;

        let checkin_url = format!("https://{}/api/v3/lesson/checkin", self.host);

        let mut bearer_token = None;
        let mut lesson_token = None;

        for _ in 0..3 {
            let response = self
                .client
                .post(&checkin_url)
                .headers(headers.clone())
                .json(&json!({
                    "source": 5,
                    "lessonId": lesson_id.to_string(),
                }))
                .send()
                .await
                .map_err(ApiError::Http)?;

            let response_headers = response.headers().clone();

            if let Some(auth_val) = response_headers
                .get("set-auth")
                .or_else(|| response_headers.get("Set-Auth"))
                && let Ok(auth_str) = auth_val.to_str()
            {
                bearer_token = Some(auth_str.to_string());
            }

            let checkin_value: Value = response.json().await.map_err(ApiError::Http)?;
            if let Ok(data) = Self::parse_api_ok(checkin_value.clone()) {
                if lesson_token.is_none() {
                    lesson_token = data
                        .get("lessonToken")
                        .and_then(Value::as_str)
                        .map(ToString::to_string);
                }
            } else {
                tracing::warn!("checkin response error: {:?}", checkin_value);
            }

            if bearer_token.is_some() && lesson_token.is_some() {
                break;
            }
            tokio::time::sleep(Duration::from_secs(1)).await;
        }

        let bearer_token = bearer_token.unwrap_or_default();
        let lesson_token = lesson_token.ok_or(ApiError::MissingField(
            "lessonToken (tried 3 times but failed)",
        ))?;
        tracing::debug!("WebSocket bearer acquired: {}", bearer_token);
        tracing::debug!("WebSocket lesson token acquired: {}", lesson_token);

        let user_url = format!("https://{}/api/v3/user/basic-info", self.host);
        let user_value: Value = self
            .client
            .get(user_url)
            .headers(headers)
            .send()
            .await
            .map_err(ApiError::Http)?
            .json()
            .await
            .map_err(ApiError::Http)?;
        let user_data = Self::parse_api_ok(user_value)?;
        let user_id = user_data
            .get("id")
            .and_then(Value::as_u64)
            .or(Some(auth.user_id))
            .filter(|id| *id != 0)
            .ok_or(ApiError::MissingField("user_id"))?;

        Ok((user_id, bearer_token, lesson_token))
    }
}

#[async_trait]
impl RainClassroomWs for YktApiPort {
    async fn connect_lesson_stream(
        &self,
        auth: &AuthContext,
        lesson_id: u64,
    ) -> Result<WsEventStream, ApiError> {
        let (user_id, bearer_token, lesson_token) =
            self.prepare_lesson_ws_auth(auth, lesson_id).await?;

        let ws_url = format!("wss://{}/wsapp/", self.host);
        let mut request = ws_url
            .into_client_request()
            .map_err(|err| ApiError::ws_request(format!("invalid ws request: {err}")))?;
        let cookie = format!("sessionid={}", auth.access_token);
        request.headers_mut().insert(
            COOKIE,
            HeaderValue::from_str(&cookie)
                .map_err(|err| ApiError::invalid_header("cookie", err))?,
        );
        request.headers_mut().insert(
            USER_AGENT,
            HeaderValue::from_str(&self.user_agent)
                .map_err(|err| ApiError::invalid_header("user-agent", err))?,
        );
        if !bearer_token.is_empty() {
            request.headers_mut().insert(
                AUTHORIZATION,
                HeaderValue::from_str(&format!("Bearer {}", bearer_token))
                    .map_err(|err| ApiError::invalid_header("authorization", err))?,
            );
        }
        // Add Host and Origin headers to simulate browser / standard client
        request.headers_mut().insert(
            "Origin",
            HeaderValue::from_str(&format!("https://{}", self.host))
                .map_err(|err| ApiError::invalid_header("origin", err))?,
        );

        let (mut socket, _) = connect_async(request)
            .await
            .map_err(|err| ApiError::ws_connect(err.to_string()))?;

        let hello = json!({
            "op": "hello",
            "userid": user_id,
            "role": "student",
            "auth": lesson_token,
            "lessonid": lesson_id.to_string(),
        })
        .to_string();
        tracing::debug!("Sending initial WS message: {}", hello);
        socket
            .send(Message::Text(hello.into()))
            .await
            .map_err(|err| ApiError::ws_send(format!("send hello failed: {err}")))?;

        let (tx, rx) = mpsc::channel::<Result<WsEventDto, ApiError>>(128);
        tokio::spawn(async move {
            while let Some(frame) = socket.next().await {
                let text = match frame {
                    Ok(Message::Text(text)) => text.to_string(),
                    Ok(Message::Binary(bin)) => String::from_utf8_lossy(&bin).to_string(),
                    Ok(_) => {
                        tracing::debug!("Received non-text/binary WS frame, continuing...");
                        continue;
                    }
                    Err(err) => {
                        let err_str = err.to_string();
                        if err_str
                            .contains("peer closed connection without sending TLS close_notify")
                        {
                            // Gracefully handle unexpected EOF caused by lack of TLS close_notify from server
                            break;
                        }
                        let _ = tx.send(Err(ApiError::ws_receive(err_str))).await;
                        break;
                    }
                };

                tracing::debug!("Received WS message: {}", text);

                let value: Value = match serde_json::from_str(&text) {
                    Ok(v) => v,
                    Err(_) => {
                        let _ = tx
                            .send(Ok(WsEventDto::Unknown {
                                raw_type: "invalid_json".to_string(),
                                raw_payload: text,
                            }))
                            .await;
                        continue;
                    }
                };
                let op = value.get("op").and_then(Value::as_str).unwrap_or("unknown");

                let mapped = match op {
                    "showpresentation" => {
                        let pres_id = value
                            .get("presentation")
                            .and_then(Value::as_str)
                            .and_then(|s| s.parse::<u64>().ok())
                            .unwrap_or(0);
                        if pres_id > 0 {
                            WsEventDto::PresentationUpdated(crate::api::PresentationUpdatedDto {
                                lesson_id,
                                presentation_id: pres_id,
                            })
                        } else {
                            WsEventDto::Unknown {
                                raw_type: op.to_string(),
                                raw_payload: text.clone(),
                            }
                        }
                    }
                    "slidenav" => {
                        if let Some(slide) = value.get("slide") {
                            let pres_id = slide
                                .get("pres")
                                .and_then(Value::as_str)
                                .and_then(|s| s.parse::<u64>().ok())
                                .unwrap_or(0);
                            let slide_id = slide
                                .get("sid")
                                .and_then(Value::as_str)
                                .and_then(|s| s.parse::<u64>().ok())
                                .unwrap_or(0);
                            let slide_index = slide.get("si").and_then(Value::as_u64).unwrap_or(0);

                            WsEventDto::SlideNavigated(crate::api::SlideNavigatedDto {
                                lesson_id,
                                presentation_id: pres_id,
                                slide_id,
                                slide_index,
                            })
                        } else {
                            WsEventDto::Unknown {
                                raw_type: op.to_string(),
                                raw_payload: text.clone(),
                            }
                        }
                    }
                    "unlockproblem" => {
                        let prob_id = value
                            .get("problem")
                            .and_then(|p| p.get("prob"))
                            .and_then(Value::as_str)
                            .and_then(|s| s.parse::<u64>().ok())
                            .unwrap_or(0);

                        if prob_id > 0 {
                            WsEventDto::ProblemUnlocked(crate::api::ProblemUnlockedDto {
                                lesson_id,
                                problem_id: prob_id,
                            })
                        } else {
                            WsEventDto::Unknown {
                                raw_type: op.to_string(),
                                raw_payload: text.clone(),
                            }
                        }
                    }
                    "probleminfo" => {
                        let pseudo_problem = json!({
                            "sid": value.get("problemid").and_then(Value::as_u64).unwrap_or(0),
                            "problemType": value.get("problemType").and_then(Value::as_i64).unwrap_or(0),
                            "title": value.get("title").and_then(Value::as_str).unwrap_or("WS 题目信息"),
                        });
                        Self::map_ws_problem(&pseudo_problem, lesson_id).unwrap_or(
                            WsEventDto::Unknown {
                                raw_type: op.to_string(),
                                raw_payload: text.clone(),
                            },
                        )
                    }
                    "checkinopened" | "checkin" => {
                        let checkin_id = value
                            .get("checkin_id")
                            .or_else(|| value.get("checkinId"))
                            .or_else(|| value.get("id"))
                            .and_then(Value::as_u64)
                            .unwrap_or(0);
                        WsEventDto::CheckinOpened(crate::api::CheckinDto {
                            lesson_id,
                            checkin_id,
                            opened_at: Utc::now(),
                        })
                    }
                    "lessonfinished" => WsEventDto::LessonEnded { lesson_id },
                    "hello" => {
                        // Handshake acknowledged, ignore
                        continue;
                    }
                    _ => WsEventDto::Unknown {
                        raw_type: op.to_string(),
                        raw_payload: text.clone(),
                    },
                };

                if tx.send(Ok(mapped)).await.is_err() {
                    break;
                }
            }
        });

        Ok(Box::pin(stream::unfold(rx, |mut rx| async {
            rx.recv().await.map(|item| (item, rx))
        })))
    }
}
fn map_problem_type(raw: &Value) -> ProblemType {
    if let Some(code) = raw.as_i64() {
        return match code {
            1 => ProblemType::SingleChoice,
            2 => ProblemType::MultipleChoice,
            3 => ProblemType::FillBlank,
            _ => ProblemType::Unknown,
        };
    }

    let text = raw.as_str().unwrap_or_default().to_ascii_lowercase();
    if text.contains("multiple") {
        ProblemType::MultipleChoice
    } else if text.contains("single") || text.contains("choice") {
        ProblemType::SingleChoice
    } else if text.contains("blank") || text.contains("fill") {
        ProblemType::FillBlank
    } else {
        ProblemType::Unknown
    }
}

fn parse_problem_options(problem: &Value) -> Vec<ProblemOption> {
    let mut options = Vec::new();
    let candidates = problem
        .get("options")
        .or_else(|| problem.get("choices"))
        .or_else(|| problem.get("optionList"))
        .or_else(|| problem.get("choiceList"));

    if let Some(Value::Array(items)) = candidates {
        for (index, item) in items.iter().enumerate() {
            match item {
                Value::Object(_) => {
                    let option_id = item
                        .get("optionId")
                        .or_else(|| item.get("option_id"))
                        .or_else(|| item.get("id"))
                        .or_else(|| item.get("key"))
                        .and_then(Value::as_str)
                        .map(ToString::to_string)
                        .unwrap_or_else(|| index.to_string());
                    let text = item
                        .get("text")
                        .or_else(|| item.get("content"))
                        .or_else(|| item.get("label"))
                        .or_else(|| item.get("value"))
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_string();
                    options.push(ProblemOption { option_id, text });
                }
                Value::String(text) => {
                    options.push(ProblemOption {
                        option_id: index.to_string(),
                        text: text.clone(),
                    });
                }
                _ => {}
            }
        }
    }

    if options.is_empty()
        && let Some(Value::Array(answers)) = problem.get("answers")
    {
        for answer in answers {
            if let Some(text) = answer.as_str() {
                options.push(ProblemOption {
                    option_id: text.to_string(),
                    text: text.to_string(),
                });
            }
        }
    }

    options
}

/// Extract correct answer IDs/values from the problem JSON.
/// Maps to Python's `problem["answers"]`.
fn parse_correct_answers(problem: &Value) -> Vec<String> {
    let mut answers = Vec::new();
    if let Some(Value::Array(items)) = problem.get("answers") {
        for item in items {
            match item {
                Value::String(s) => answers.push(s.clone()),
                Value::Number(n) => answers.push(n.to_string()),
                Value::Bool(b) => answers.push(b.to_string()),
                _ => {}
            }
        }
    }
    answers
}

/// Extract fill-blank answer slots from the problem JSON.
/// Maps to Python's `problem["blanks"]`, where each blank has `["answers"]`.
fn parse_blanks(problem: &Value) -> Vec<BlankAnswer> {
    let mut blanks = Vec::new();
    if let Some(Value::Array(items)) = problem.get("blanks") {
        for item in items {
            let mut accepted = Vec::new();
            if let Some(Value::Array(answers)) = item.get("answers") {
                for answer in answers {
                    match answer {
                        Value::String(s) => accepted.push(s.clone()),
                        Value::Number(n) => accepted.push(n.to_string()),
                        _ => {}
                    }
                }
            }
            blanks.push(BlankAnswer {
                accepted_values: accepted,
            });
        }
    }
    blanks
}

/// Extract the time limit in seconds from the problem JSON.
/// Returns None for unlimited (-1) problems.
fn parse_limit(problem: &Value) -> Option<i64> {
    problem
        .get("limit")
        .and_then(Value::as_i64)
        .and_then(|v| if v == -1 { None } else { Some(v) })
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
        self.update_qr_state(&scene_id, QrSceneState::Pending);

        let ws_url = format!("wss://{}/wsapp/", self.host);
        let scene_for_task = scene_id.clone();
        let host = self.host.clone();
        let user_agent = self.user_agent.clone();
        let client = self.client.clone();
        let states = Arc::clone(&self.qr_states);
        let state_notify = Arc::clone(&self.qr_state_notify);
        let (bootstrap_tx, bootstrap_rx) =
            oneshot::channel::<Result<QrLoginBootstrap, ApiPortError>>();

        tokio::spawn(async move {
            let mut bootstrap_tx = Some(bootstrap_tx);
            let (mut socket, _) = match connect_async(&ws_url).await {
                Ok(pair) => pair,
                Err(err) => {
                    Self::update_qr_state_shared(
                        &states,
                        &state_notify,
                        &scene_for_task,
                        QrSceneState::Rejected,
                    );
                    if let Some(sender) = bootstrap_tx.take() {
                        let _ = sender.send(Err(ApiPortError::request("connect wsapp", err)));
                    }
                    return;
                }
            };

            let req = json!({
                "op": "requestlogin",
                "role": "web",
                "version": 1.4,
                "type": "qrcode",
                "from": "web",
            })
            .to_string();

            if let Err(err) = socket.send(Message::Text(req.into())).await {
                Self::update_qr_state_shared(
                    &states,
                    &state_notify,
                    &scene_for_task,
                    QrSceneState::Rejected,
                );
                if let Some(sender) = bootstrap_tx.take() {
                    let _ = sender.send(Err(ApiPortError::request("send requestlogin", err)));
                }
                return;
            }

            let mut bootstrap_sent = false;
            while let Some(frame) = socket.next().await {
                let text = match frame {
                    Ok(Message::Text(text)) => text.to_string(),
                    Ok(Message::Binary(bin)) => String::from_utf8_lossy(&bin).to_string(),
                    Ok(_) => continue,
                    Err(err) => {
                        Self::update_qr_state_shared(
                            &states,
                            &state_notify,
                            &scene_for_task,
                            QrSceneState::Rejected,
                        );
                        if !bootstrap_sent && let Some(sender) = bootstrap_tx.take() {
                            let _ = sender.send(Err(ApiPortError::request("receive wsapp", err)));
                        }
                        return;
                    }
                };

                let value: Value = match serde_json::from_str(&text) {
                    Ok(v) => v,
                    Err(_) => continue,
                };
                let op = value.get("op").and_then(Value::as_str).unwrap_or_default();

                if op == "requestlogin" && !bootstrap_sent {
                    let ticket = value
                        .get("ticket")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_string();
                    if ticket.is_empty() {
                        Self::update_qr_state_shared(
                            &states,
                            &state_notify,
                            &scene_for_task,
                            QrSceneState::Rejected,
                        );
                        if let Some(sender) = bootstrap_tx.take() {
                            let _ = sender
                                .send(Err(ApiPortError::protocol("requestlogin missing ticket")));
                        }
                        return;
                    }

                    bootstrap_sent = true;
                    if let Some(sender) = bootstrap_tx.take() {
                        let _ = sender.send(Ok(QrLoginBootstrap {
                            scene_id: scene_for_task.clone(),
                            token: ticket.clone(),
                            qr_svg: ticket,
                        }));
                    }
                    continue;
                }

                if op == "loginsuccess" {
                    let user_id = value
                        .get("UserID")
                        .and_then(Value::as_u64)
                        .unwrap_or_default();
                    let auth = value
                        .get("Auth")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_string();

                    if user_id == 0 || auth.is_empty() {
                        Self::update_qr_state_shared(
                            &states,
                            &state_notify,
                            &scene_for_task,
                            QrSceneState::Rejected,
                        );
                        return;
                    }

                    let login_url = format!("https://{host}/pc/web_login");
                    let response = match client
                        .post(login_url)
                        .header(USER_AGENT, &user_agent)
                        .json(&json!({"UserID": user_id, "Auth": auth}))
                        .send()
                        .await
                    {
                        Ok(res) => res,
                        Err(_) => {
                            Self::update_qr_state_shared(
                                &states,
                                &state_notify,
                                &scene_for_task,
                                QrSceneState::Rejected,
                            );
                            return;
                        }
                    };

                    if !response.status().is_success() {
                        Self::update_qr_state_shared(
                            &states,
                            &state_notify,
                            &scene_for_task,
                            QrSceneState::Rejected,
                        );
                        return;
                    }

                    let sessionid = match Self::extract_session_id(response.headers()) {
                        Some(cookie) => cookie,
                        None => {
                            Self::update_qr_state_shared(
                                &states,
                                &state_notify,
                                &scene_for_task,
                                QrSceneState::Rejected,
                            );
                            return;
                        }
                    };

                    let session = AuthSession {
                        user_id,
                        access_token: sessionid,
                        refresh_token: None,
                        expires_at_unix_ms: None,
                    };

                    Self::update_qr_state_shared(
                        &states,
                        &state_notify,
                        &scene_for_task,
                        QrSceneState::Confirmed(session),
                    );
                    return;
                }
            }

            Self::update_qr_state_shared(
                &states,
                &state_notify,
                &scene_for_task,
                QrSceneState::Expired,
            );
            if !bootstrap_sent && let Some(sender) = bootstrap_tx.take() {
                let _ = sender.send(Err(ApiPortError::protocol("wsapp closed before qr ticket")));
            }
        });

        bootstrap_rx
            .await
            .map_err(|_| ApiPortError::request("qr bootstrap channel", "dropped"))?
    }

    async fn poll_qr_login(&self, scene_id: &str) -> Result<QrLoginProgress, ApiPortError> {
        let mut states = self.qr_states.lock().expect("qr state poisoned");
        let Some(state) = states.get(scene_id).cloned() else {
            return Err(ApiPortError::protocol(format!(
                "unknown scene_id: {scene_id}"
            )));
        };

        match state {
            QrSceneState::Pending => Ok(QrLoginProgress::Pending),
            QrSceneState::Expired => {
                states.remove(scene_id);
                Ok(QrLoginProgress::Expired)
            }
            QrSceneState::Rejected => {
                states.remove(scene_id);
                Ok(QrLoginProgress::Rejected)
            }
            QrSceneState::Confirmed(session) => {
                states.remove(scene_id);
                Ok(QrLoginProgress::Confirmed(session))
            }
        }
    }

    async fn wait_qr_login(
        &self,
        scene_id: &str,
        timeout_secs: u64,
    ) -> Result<QrLoginProgress, ApiPortError> {
        let timeout_secs = timeout_secs.max(1);
        let deadline = tokio::time::Instant::now() + Duration::from_secs(timeout_secs);

        loop {
            let maybe_progress = {
                let mut states = self.qr_states.lock().expect("qr state poisoned");
                let Some(state) = states.get(scene_id).cloned() else {
                    return Err(ApiPortError::protocol(format!(
                        "unknown scene_id: {scene_id}"
                    )));
                };

                match state {
                    QrSceneState::Pending => None,
                    QrSceneState::Expired => {
                        states.remove(scene_id);
                        Some(QrLoginProgress::Expired)
                    }
                    QrSceneState::Rejected => {
                        states.remove(scene_id);
                        Some(QrLoginProgress::Rejected)
                    }
                    QrSceneState::Confirmed(session) => {
                        states.remove(scene_id);
                        Some(QrLoginProgress::Confirmed(session))
                    }
                }
            };

            if let Some(progress) = maybe_progress {
                return Ok(progress);
            }

            let now = tokio::time::Instant::now();
            if now >= deadline {
                return Ok(QrLoginProgress::Pending);
            }

            let wait_for = deadline.saturating_duration_since(now);
            if tokio::time::timeout(wait_for, self.qr_state_notify.notified())
                .await
                .is_err()
            {
                return Ok(QrLoginProgress::Pending);
            }
        }
    }

    async fn refresh_session(&self, refresh_token: &str) -> Result<AuthSession, ApiPortError> {
        let session = AuthSession {
            user_id: 0,
            access_token: refresh_token.to_string(),
            refresh_token: None,
            expires_at_unix_ms: None,
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

        let data = Self::parse_api_ok(value).map_err(ApiPortError::protocol)?;
        let user_id = data.get("id").and_then(Value::as_u64).unwrap_or(0);
        if user_id == 0 {
            return Err(ApiPortError::protocol("refresh basic-info missing user id"));
        }

        Ok(AuthSession {
            user_id,
            access_token: refresh_token.to_string(),
            refresh_token: None,
            expires_at_unix_ms: None,
        })
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
