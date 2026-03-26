use std::time::Duration;

use async_trait::async_trait;
use chrono::Utc;
use futures_util::{SinkExt, StreamExt};
use reqwest::header::{AUTHORIZATION, COOKIE, HeaderValue, USER_AGENT};
use serde_json::{Value, json};
use tokio::sync::mpsc;
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;

use crate::api::{ApiError, AuthContext, RainClassroomWs, WsEventDto, WsEventStream};

use super::YktApiPort;

pub(super) fn map_ws_problem(problem_value: &Value, lesson_id: u64) -> Option<WsEventDto> {
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

impl YktApiPort {
    async fn prepare_lesson_ws_auth(
        &self,
        auth: &AuthContext,
        lesson_id: u64,
    ) -> Result<(u64, String, String), ApiError> {
        let session = rca_core::auth::AuthSession {
            user_id: auth.user_id,
            access_token: auth.access_token.clone(),
            refresh_token: auth.refresh_token.clone(),
            expires_at_unix_ms: None,
            csrf_token: None,
            original_id: None,
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
            if let Ok(data) = YktApiPort::parse_api_ok(checkin_value.clone()) {
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

        fn redact_token(token: &str) -> String {
            let t = token.trim();
            if t.is_empty() {
                return String::new();
            }
            if t.len() <= 12 {
                return format!("{}***", &t[..t.len().min(4)]);
            }
            let head = &t[..4];
            let tail = &t[t.len() - 4..];
            format!("{head}***{tail}")
        }

        tracing::debug!(
            bearer = %redact_token(&bearer_token),
            bearer_len = bearer_token.len(),
            lesson_token = %redact_token(&lesson_token),
            lesson_token_len = lesson_token.len(),
            "websocket auth acquired"
        );

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
        let user_data = YktApiPort::parse_api_ok(user_value)?;
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

        let span = tracing::debug_span!(
            target: "rca_infra.ws",
            "connect_lesson_stream",
            host = %self.host,
            lesson_id = lesson_id,
            user_id = user_id,
            has_bearer = !bearer_token.is_empty()
        );
        let _enter = span.enter();

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
                HeaderValue::from_str(&format!("Bearer {bearer_token}"))
                    .map_err(|err| ApiError::invalid_header("authorization", err))?,
            );
        }
        request.headers_mut().insert(
            "Origin",
            HeaderValue::from_str(&format!("https://{}", self.host))
                .map_err(|err| ApiError::invalid_header("origin", err))?,
        );

        tracing::debug!("connecting lesson ws");
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

        tracing::debug!(
            op = "hello",
            bytes = hello.len(),
            "sending initial ws message"
        );
        socket
            .send(Message::Text(hello.into()))
            .await
            .map_err(|err| ApiError::ws_send(format!("send hello failed: {err}")))?;
        tracing::debug!("lesson ws connected");

        let (tx, rx) = mpsc::channel::<Result<WsEventDto, ApiError>>(128);
        tokio::spawn(async move {
            while let Some(frame) = socket.next().await {
                let text = match frame {
                    Ok(Message::Text(text)) => text.to_string(),
                    Ok(Message::Binary(bin)) => String::from_utf8_lossy(&bin).to_string(),
                    Ok(Message::Ping(_)) => continue,
                    Ok(Message::Pong(_)) => continue,
                    Ok(Message::Close(_)) => break,
                    Ok(_) => {
                        tracing::trace!("Received non-data WS frame, continuing...");
                        continue;
                    }
                    Err(err) => {
                        let err_str = err.to_string();
                        if err_str
                            .contains("peer closed connection without sending TLS close_notify")
                        {
                            break;
                        }
                        let _ = tx.send(Err(ApiError::ws_receive(err_str))).await;
                        break;
                    }
                };

                let value: Value = match serde_json::from_str(&text) {
                    Ok(v) => v,
                    Err(_) => {
                        tracing::debug!(raw_len = text.len(), "invalid ws json payload");
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
                tracing::trace!(op = op, raw_len = text.len(), "ws message received");

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
                            "title": value
                                .get("title")
                                .and_then(Value::as_str)
                                .unwrap_or("WS 题目信息"),
                        });
                        map_ws_problem(&pseudo_problem, lesson_id).unwrap_or(WsEventDto::Unknown {
                            raw_type: op.to_string(),
                            raw_payload: text.clone(),
                        })
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
                        let mut unique_pres_ids = std::collections::HashSet::new();

                        if let Some(root_pres_id) = value
                            .get("presentation")
                            .and_then(Value::as_str)
                            .and_then(|s| s.parse::<u64>().ok())
                            && root_pres_id > 0
                        {
                            unique_pres_ids.insert(root_pres_id);
                        }

                        if let Some(timeline) = value.get("timeline").and_then(Value::as_array) {
                            for item in timeline {
                                if let Some(pres_id) = item
                                    .get("pres")
                                    .and_then(Value::as_str)
                                    .and_then(|s| s.parse::<u64>().ok())
                                    && pres_id > 0
                                {
                                    unique_pres_ids.insert(pres_id);
                                }
                            }
                        }

                        for pres_id in unique_pres_ids {
                            let _ = tx
                                .send(Ok(WsEventDto::PresentationUpdated(
                                    crate::api::PresentationUpdatedDto {
                                        lesson_id,
                                        presentation_id: pres_id,
                                    },
                                )))
                                .await;
                        }
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

        Ok(Box::pin(futures_util::stream::unfold(rx, |mut rx| async {
            rx.recv().await.map(|item| (item, rx))
        })))
    }
}
