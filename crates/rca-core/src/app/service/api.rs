use async_trait::async_trait;

use tokio::sync::mpsc;

use crate::app::{AppCommand, AppError, AppEvent, AppQuery, AppService};
use crate::auth::AuthSession;
use crate::auth::AuthState;

use super::AppServiceImpl;

#[async_trait]
impl AppService for AppServiceImpl {
    async fn handle_command(&self, cmd: AppCommand) -> Result<(), AppError> {
        match cmd {
            AppCommand::LoadConfig => {
                let config = self
                    .deps
                    .config_store
                    .load_config()
                    .await
                    .map_err(AppError::from)?;
                self.with_inner_mut(|inner| inner.config = config).await;
                self.emit_state_changed().await;
                Ok(())
            }
            AppCommand::RestoreSession => {
                let session = self
                    .deps
                    .session_store
                    .load_session()
                    .await
                    .map_err(AppError::from)?;

                match session {
                    Some(session) => {
                        self.set_auth_logged_in(session.user_id).await;
                        self.clear_last_error().await;
                    }
                    None => {
                        self.set_auth_logged_out().await;
                    }
                }

                self.emit_state_changed().await;
                Ok(())
            }
            AppCommand::RefreshSession => {
                let session = self
                    .deps
                    .session_store
                    .load_session()
                    .await
                    .map_err(AppError::from)?
                    .ok_or_else(|| {
                        AppError::InvalidCommand("session missing for refresh".to_string())
                    })?;

                let refresh_token = session
                    .refresh_token
                    .clone()
                    .unwrap_or_else(|| session.access_token.clone());

                let refreshed = self
                    .deps
                    .api
                    .refresh_session(&refresh_token)
                    .await
                    .map_err(AppError::from)?;

                let refreshed = AuthSession {
                    csrf_token: refreshed.csrf_token.or(session.csrf_token.clone()),
                    original_id: refreshed.original_id.or(session.original_id.clone()),
                    ..refreshed
                };

                self.deps
                    .session_store
                    .save_session(&refreshed)
                    .await
                    .map_err(AppError::from)?;

                self.set_auth_logged_in(refreshed.user_id).await;
                self.clear_last_error().await;

                self.emit_state_changed().await;
                Ok(())
            }
            AppCommand::LoginByQr => {
                let bootstrap = self
                    .deps
                    .api
                    .start_qr_login()
                    .await
                    .map_err(AppError::from)?;
                self.set_auth_waiting_qr(bootstrap.scene_id, bootstrap.token)
                    .await;
                self.clear_last_error().await;
                self.emit_state_changed().await;
                Ok(())
            }
            AppCommand::PollLogin { scene_id } => {
                let progress = self
                    .deps
                    .api
                    .poll_qr_login(&scene_id)
                    .await
                    .map_err(AppError::from)?;
                self.apply_qr_login_progress(scene_id, progress).await
            }
            AppCommand::AwaitLogin {
                scene_id,
                timeout_secs,
            } => {
                let expected_scene = self
                    .with_inner(|inner| match &inner.app_state.auth_state {
                        AuthState::WaitingQrScan { scene_id, .. } => Some(scene_id.clone()),
                        _ => None,
                    })
                    .await;
                if expected_scene.as_deref() != Some(scene_id.as_str()) {
                    return Err(AppError::InvalidCommand(
                        "await login scene_id does not match active QR login".to_string(),
                    ));
                }

                let progress = match self.deps.api.wait_qr_login(&scene_id, timeout_secs).await {
                    Ok(progress) => progress,
                    Err(err) => {
                        self.set_last_error(err.to_string()).await;
                        self.emit_state_changed().await;
                        return Err(AppError::from(err));
                    }
                };
                self.apply_qr_login_progress(scene_id, progress).await
            }
            AppCommand::Logout => {
                self.stop_monitor_engine().await;
                self.deps
                    .session_store
                    .clear_session()
                    .await
                    .map_err(AppError::from)?;
                self.set_auth_logged_out().await;
                self.set_monitor_running(false).await;
                self.emit_state_changed().await;
                Ok(())
            }
            AppCommand::StartMonitor => {
                let should_start = self
                    .with_inner(|inner| {
                        if matches!(inner.app_state.auth_state, AuthState::LoggedOut) {
                            return Err(AppError::InvalidCommand(
                                "cannot start monitor before login".to_string(),
                            ));
                        }
                        if inner.app_state.monitor_running {
                            return Ok(false);
                        }
                        Ok(true)
                    })
                    .await?;

                if !should_start {
                    return Ok(());
                }

                self.set_monitor_running(true).await;
                self.clear_last_error().await;
                self.start_background_monitor().await;
                self.emit_state_changed().await;
                Ok(())
            }
            AppCommand::StopMonitor => {
                self.stop_monitor_engine().await;
                self.set_monitor_running(false).await;
                self.emit_state_changed().await;
                Ok(())
            }
            AppCommand::CheckUpdate => {
                let update = self
                    .deps
                    .update_checker
                    .check_latest(env!("CARGO_PKG_VERSION"))
                    .await
                    .map_err(AppError::from)?;
                if let Some(info) = update {
                    self.emit_event(AppEvent::UpdateAvailable {
                        version: info.latest_version,
                        url: info.release_url,
                    })
                    .await;
                }
                Ok(())
            }
            AppCommand::SaveConfig { config } => {
                self.deps
                    .config_store
                    .save_config(&config)
                    .await
                    .map_err(AppError::from)?;
                self.with_inner_mut(|inner| inner.config = config).await;
                self.emit_state_changed().await;
                Ok(())
            }
            AppCommand::DownloadPresentation {
                presentation_id,
                lesson_id,
                save_dir,
            } => {
                let session = self
                    .deps
                    .session_store
                    .load_session()
                    .await
                    .map_err(AppError::from)?
                    .ok_or_else(|| {
                        AppError::InvalidCommand("必须先登录才能下载 PPT".to_string())
                    })?;
                let path = self
                    .deps
                    .api
                    .download_presentation(&session, presentation_id, lesson_id, &save_dir)
                    .await
                    .map_err(AppError::from)?;
                self.emit_event(super::AppEvent::Notification(crate::app::AppNotification {
                    title: "PPT 下载成功".to_string(),
                    body: format!("已保存至: {}", path.display()),
                }))
                .await;
                Ok(())
            }
        }
    }

    async fn handle_query(&self, query: AppQuery) -> Result<crate::app::AppQueryResult, AppError> {
        self.query_impl(query).await
    }

    async fn subscribe_events(&self) -> mpsc::Receiver<AppEvent> {
        self.subscribe_events_impl().await
    }
}
