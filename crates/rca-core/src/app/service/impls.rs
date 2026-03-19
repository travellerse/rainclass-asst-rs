use std::sync::{Arc, Mutex};

use tokio::sync::mpsc;
use tokio::time::Duration;

use crate::app::AppEvent;
use crate::app::AppNotification;
use crate::app::AppState;
use crate::app::notify_event_keys;
use crate::auth::AuthState;
use crate::auth::QrLoginProgress;
use crate::monitor::CoreEvent;

use super::InnerState;
use super::background;
use super::{AppServiceImpl, CoreAppDeps};

fn lock_inner<'a>(inner: &'a Arc<Mutex<InnerState>>) -> std::sync::MutexGuard<'a, InnerState> {
    inner.lock().expect("core app state poisoned")
}

impl AppServiceImpl {
    pub fn new(deps: CoreAppDeps, initial_config: crate::app::AppConfigDto) -> Self {
        let inner = Arc::new(Mutex::new(InnerState {
            app_state: AppState {
                auth_state: AuthState::LoggedOut,
                monitor_running: false,
                current_lessons: Vec::new(),
                recent_events: Vec::new(),
                last_error: None,
            },
            config: initial_config,
            subscribers: Vec::new(),
        }));
        Self {
            deps,
            inner,
            background: Mutex::new(None),
        }
    }

    pub fn new_started(deps: CoreAppDeps, initial_config: crate::app::AppConfigDto) -> Self {
        let service = Self::new(deps, initial_config);
        service.start_background_tasks();
        service
    }

    pub fn start_background_tasks(&self) {
        background::start_background_tasks(self);
    }

    pub fn stop_background_tasks(&self) {
        background::stop_background_tasks(self);
    }

    pub(super) fn with_inner<R>(&self, f: impl FnOnce(&InnerState) -> R) -> R {
        let guard = lock_inner(&self.inner);
        f(&guard)
    }

    pub(super) fn with_inner_mut<R>(&self, f: impl FnOnce(&mut InnerState) -> R) -> R {
        let mut guard = lock_inner(&self.inner);
        f(&mut guard)
    }

    pub(super) fn set_auth_logged_in(&self, user_id: u64) {
        self.with_inner_mut(|inner| {
            inner.app_state.auth_state = AuthState::LoggedIn { user_id };
        });
    }

    pub(super) fn set_auth_logged_out(&self) {
        self.with_inner_mut(|inner| {
            inner.app_state.auth_state = AuthState::LoggedOut;
        });
    }

    pub(super) fn set_auth_waiting_qr(&self, scene_id: String, token: String) {
        self.with_inner_mut(|inner| {
            inner.app_state.auth_state = AuthState::WaitingQrScan { scene_id, token };
        });
    }

    pub(super) fn set_auth_failed(&self, reason: String) {
        self.with_inner_mut(|inner| {
            inner.app_state.auth_state = AuthState::Failed { reason };
        });
    }

    pub(super) fn set_monitor_running(&self, running: bool) {
        self.with_inner_mut(|inner| {
            inner.app_state.monitor_running = running;
        });
    }

    pub(super) fn clear_last_error(&self) {
        self.with_inner_mut(|inner| {
            inner.app_state.last_error = None;
        });
    }

    pub(super) fn set_last_error(&self, error: String) {
        self.with_inner_mut(|inner| {
            inner.app_state.last_error = Some(error);
        });
    }

    pub(super) async fn emit_event(&self, event: AppEvent) {
        Self::emit_event_with_inner(&self.inner, event).await;
    }

    pub(super) async fn emit_event_with_inner(inner: &Arc<Mutex<InnerState>>, event: AppEvent) {
        let subscribers = {
            let guard = lock_inner(inner);
            guard.subscribers.clone()
        };

        let mut active_subscribers = Vec::with_capacity(subscribers.len());
        for sender in subscribers {
            if sender.send(event.clone()).await.is_ok() {
                active_subscribers.push(sender);
            }
        }

        let mut guard = lock_inner(inner);
        guard.subscribers = active_subscribers;
    }

    pub(super) async fn emit_state_changed(&self) {
        Self::emit_state_changed_with_inner(&self.inner).await;
    }

    pub(super) async fn emit_state_changed_with_inner(inner: &Arc<Mutex<InnerState>>) {
        let snapshot = {
            let guard = lock_inner(inner);
            guard.app_state.clone()
        };
        Self::emit_event_with_inner(inner, AppEvent::StateChanged(snapshot)).await;
    }

    pub(super) fn append_recent_event(inner: &mut InnerState, event: CoreEvent) {
        inner.app_state.recent_events.push(event);
        if inner.app_state.recent_events.len() > super::MAX_RECENT_EVENTS {
            let drain_count = inner.app_state.recent_events.len() - super::MAX_RECENT_EVENTS;
            inner.app_state.recent_events.drain(0..drain_count);
        }
    }

    pub(super) async fn apply_qr_login_progress(
        &self,
        _scene_id: String,
        progress: QrLoginProgress,
    ) -> Result<(), crate::app::AppError> {
        let maybe_notify = match progress {
            QrLoginProgress::Pending => None,
            QrLoginProgress::Confirmed(session) => {
                self.deps
                    .session_store
                    .save_session(&session)
                    .await
                    .map_err(crate::app::AppError::from)?;
                self.set_auth_logged_in(session.user_id);
                Some(AppNotification {
                    title: "登录成功".to_string(),
                    body: "会话已建立并保存。".to_string(),
                })
            }
            QrLoginProgress::Expired => {
                self.set_auth_failed("二维码已过期".to_string());
                None
            }
            QrLoginProgress::Rejected => {
                self.set_auth_failed("登录被拒绝".to_string());
                None
            }
        };

        if let Some(message) = maybe_notify {
            let config = self.with_inner(|inner| inner.config.clone());
            if config.notify_enabled
                && Self::notify_event_enabled(&config, notify_event_keys::LOGIN_SUCCESS)
            {
                self.deps
                    .notifier
                    .notify(message.clone())
                    .await
                    .map_err(crate::app::AppError::from)?;
                self.emit_event(AppEvent::Notification(message)).await;
            }
        }
        self.emit_state_changed().await;
        Ok(())
    }

    pub(super) fn notify_event_enabled(config: &crate::app::AppConfigDto, event_key: &str) -> bool {
        config.notify_events.get(event_key).copied().unwrap_or(true)
    }

    pub(super) async fn stop_monitor_engine(&self) {
        if let Err(e) = self
            .deps
            .monitor_engine
            .stop(crate::monitor::MonitorHandle {
                task_id: crate::monitor::MonitorTaskId(1),
            })
            .await
        {
            tracing::error!("Failed to stop monitor engine: {}", e);
        }
    }

    pub(super) async fn start_background_monitor(&self) {
        let session = match self.deps.session_store.load_session().await {
            Ok(Some(s)) => s,
            _ => return,
        };
        let config = self.with_inner(|inner| inner.config.clone());
        let monitor_cfg = crate::monitor::MonitorConfig {
            poll_interval: Duration::from_secs(config.monitor_interval_secs),
            ws_reconnect_backoff_base: Duration::from_secs(5),
            ws_reconnect_backoff_max: Duration::from_secs(30),
            max_parallel_lessons: 10,
            auto_answer_enabled: config.auto_answer_enabled,
            auto_answer_random_guess: config.auto_answer_random_guess,
            auto_checkin_enabled: config.auto_checkin_enabled,
            auto_danmu_enabled: config.auto_danmu_enabled,
            danmu_threshold: config.danmu_threshold,
            delay_strategy: crate::monitor::DelayStrategy::from_type_code(
                config.answer_delay_type,
                config.answer_delay_custom_percent,
            ),
        };

        if let Err(e) = self.deps.monitor_engine.start(session, monitor_cfg).await {
            tracing::error!("Failed to start monitor engine: {}", e);
            self.set_last_error(e.to_string());
            self.set_monitor_running(false);
        }
    }

    pub(super) fn subscribe_events_impl(&self) -> mpsc::Receiver<AppEvent> {
        let (tx, rx) = mpsc::channel(128);
        self.with_inner_mut(|inner| inner.subscribers.push(tx));
        rx
    }

    pub(super) fn query_impl(
        &self,
        query: crate::app::AppQuery,
    ) -> Result<crate::app::AppQueryResult, crate::app::AppError> {
        self.with_inner(|inner| match query {
            crate::app::AppQuery::GetAppState => {
                Ok(crate::app::AppQueryResult::State(inner.app_state.clone()))
            }
            crate::app::AppQuery::GetConfig => {
                Ok(crate::app::AppQueryResult::Config(inner.config.clone()))
            }
            crate::app::AppQuery::GetRecentEvents { limit } => {
                let mut events = inner.app_state.recent_events.clone();
                if events.len() > limit {
                    events = events.split_off(events.len() - limit);
                }
                Ok(crate::app::AppQueryResult::Events(events))
            }
        })
    }
}
