use futures_util::{SinkExt, StreamExt};
use reqwest::header::USER_AGENT;
use serde_json::{Value, json};
use tokio::sync::oneshot;
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::Message;

use rca_core::app::ports::ApiPortError;
use rca_core::auth::{AuthSession, QrLoginBootstrap};

use super::{YktApiPort, qr_state};

pub(super) async fn start_qr_login(
    port: &YktApiPort,
    qr_state: qr_state::QrStateStore,
    scene_id: String,
) -> Result<QrLoginBootstrap, ApiPortError> {
    let ws_url = format!("wss://{}/wsapp/", port.host);
    let scene_for_task = scene_id.clone();
    let host = port.host.clone();
    let user_agent = port.user_agent.clone();
    let client = port.client.clone();
    let (bootstrap_tx, bootstrap_rx) = oneshot::channel::<Result<QrLoginBootstrap, ApiPortError>>();

    tokio::spawn(async move {
        let span = tracing::debug_span!(
            target: "rca_infra.api",
            "qr_login_task",
            host = %host,
            scene_id = %scene_for_task
        );
        let _enter = span.enter();

        let mut bootstrap_tx = Some(bootstrap_tx);
        tracing::debug!(ws_url = %ws_url, "connecting wsapp");
        let (mut socket, _) = match connect_async(&ws_url).await {
            Ok(pair) => pair,
            Err(err) => {
                qr_state.set(&scene_for_task, qr_state::QrSceneState::Rejected);
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

        tracing::debug!("requesting qr ticket");
        if let Err(err) = socket.send(Message::Text(req.into())).await {
            qr_state.set(&scene_for_task, qr_state::QrSceneState::Rejected);
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
                    qr_state.set(&scene_for_task, qr_state::QrSceneState::Rejected);
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
                    qr_state.set(&scene_for_task, qr_state::QrSceneState::Rejected);
                    if let Some(sender) = bootstrap_tx.take() {
                        let _ =
                            sender.send(Err(ApiPortError::protocol("requestlogin missing ticket")));
                    }
                    return;
                }

                bootstrap_sent = true;
                tracing::debug!("qr ticket received");
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
                    qr_state.set(&scene_for_task, qr_state::QrSceneState::Rejected);
                    return;
                }

                let login_url = format!("https://{host}/pc/web_login");
                tracing::debug!(user_id = user_id, "exchanging web_login session");
                let response = match client
                    .post(login_url)
                    .header(USER_AGENT, &user_agent)
                    .json(&json!({"UserID": user_id, "Auth": auth}))
                    .send()
                    .await
                {
                    Ok(res) => res,
                    Err(_) => {
                        qr_state.set(&scene_for_task, qr_state::QrSceneState::Rejected);
                        return;
                    }
                };

                if !response.status().is_success() {
                    tracing::warn!(http_status = %response.status(), "web_login failed");
                    qr_state.set(&scene_for_task, qr_state::QrSceneState::Rejected);
                    return;
                }

                let sessionid = match YktApiPort::extract_session_id(response.headers()) {
                    Some(cookie) => cookie,
                    None => {
                        tracing::warn!("web_login succeeded but sessionid missing");
                        qr_state.set(&scene_for_task, qr_state::QrSceneState::Rejected);
                        return;
                    }
                };

                let session = AuthSession {
                    user_id,
                    access_token: sessionid,
                    refresh_token: None,
                    expires_at_unix_ms: None,
                };

                qr_state.set(&scene_for_task, qr_state::QrSceneState::Confirmed(session));
                tracing::debug!(user_id = user_id, "login confirmed");
                return;
            }
        }

        qr_state.set(&scene_for_task, qr_state::QrSceneState::Expired);
        if !bootstrap_sent && let Some(sender) = bootstrap_tx.take() {
            let _ = sender.send(Err(ApiPortError::protocol("wsapp closed before qr ticket")));
        }
    });

    bootstrap_rx
        .await
        .map_err(|_| ApiPortError::request("qr bootstrap channel", "dropped"))?
}
