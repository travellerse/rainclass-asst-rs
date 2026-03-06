use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use tokio::sync::mpsc;
use tokio::time::Duration;

use crate::app::ports::{
    ApiPort, ConfigStorePort, NotifierPort, SessionStorePort, UpdateCheckerPort,
};
use crate::app::{
    AppCommand, AppConfigDto, AppError, AppEvent, AppNotification, AppQuery, AppQueryResult,
    AppService, AppState,
};
use crate::auth::{AuthState, QrLoginProgress};
use crate::monitor::CoreEvent;

const MAX_RECENT_EVENTS: usize = 200;

#[derive(Clone)]
pub struct CoreAppDeps {
    pub api: Arc<dyn ApiPort>,
    pub config_store: Arc<dyn ConfigStorePort>,
    pub session_store: Arc<dyn SessionStorePort>,
    pub notifier: Arc<dyn NotifierPort>,
    pub update_checker: Arc<dyn UpdateCheckerPort>,
    pub monitor_engine: Arc<dyn crate::monitor::MonitorEngine>,
}

struct InnerState {
    app_state: AppState,
    config: AppConfigDto,
    subscribers: Vec<mpsc::Sender<AppEvent>>,
}

pub struct CoreAppService {
    deps: CoreAppDeps,
    inner: Arc<Mutex<InnerState>>,
}

impl CoreAppService {
    pub fn new(deps: CoreAppDeps, initial_config: AppConfigDto) -> Self {
        let mut rx = deps.monitor_engine.subscribe_events();

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

        let notifier = deps.notifier.clone();
        let inner_clone = inner.clone();
        tokio::spawn(async move {
            while let Ok(event) = rx.recv().await {
                {
                    let mut guard = inner_clone.lock().expect("core app state poisoned");
                    Self::append_recent_event(&mut guard, event.clone());
                    if let CoreEvent::MonitorStopped { .. } = event {
                        guard.app_state.monitor_running = false;
                    }

                    if guard.config.notify_enabled {
                        let maybe_notify = match &event {
                            CoreEvent::AutoAnswerSubmitted {
                                lesson_id,
                                problem_id,
                            } => Some(AppNotification {
                                title: "自动答题".to_string(),
                                body: format!(
                                    "已成功提交自动答题！(课程 {}, 题目 {})",
                                    lesson_id.0.get(),
                                    problem_id.0.get()
                                ),
                            }),
                            CoreEvent::AutoCheckinSubmitted {
                                lesson_id,
                                checkin_id,
                            } => Some(AppNotification {
                                title: "自动签到".to_string(),
                                body: format!(
                                    "已成功自动签到！(课程 {}, 签到 {})",
                                    lesson_id.0.get(),
                                    checkin_id.0.get()
                                ),
                            }),
                            CoreEvent::CallPaused {
                                lesson_id,
                                target_name,
                            } => Some(AppNotification {
                                title: "老师正在点名".to_string(),
                                body: format!(
                                    "老师正在点名：{}！(课程 {})",
                                    target_name,
                                    lesson_id.0.get()
                                ),
                            }),
                            _ => None,
                        };

                        if let Some(msg) = maybe_notify {
                            let notifier_clone = notifier.clone();
                            tokio::spawn(async move {
                                let _ = notifier_clone.notify(msg).await;
                            });
                        }
                    }
                }
                Self::emit_state_changed_with_inner(&inner_clone).await;
            }
        });

        Self { deps, inner }
    }

    async fn emit_event(&self, event: AppEvent) {
        Self::emit_event_with_inner(&self.inner, event).await;
    }

    async fn emit_event_with_inner(inner: &Arc<Mutex<InnerState>>, event: AppEvent) {
        let subscribers = {
            let guard = inner.lock().expect("core app state poisoned");
            guard.subscribers.clone()
        };

        let mut active_subscribers = Vec::with_capacity(subscribers.len());
        for sender in subscribers {
            if sender.send(event.clone()).await.is_ok() {
                active_subscribers.push(sender);
            }
        }

        let mut guard = inner.lock().expect("core app state poisoned");
        guard.subscribers = active_subscribers;
    }

    async fn emit_state_changed(&self) {
        Self::emit_state_changed_with_inner(&self.inner).await;
    }

    async fn emit_state_changed_with_inner(inner: &Arc<Mutex<InnerState>>) {
        let snapshot = {
            let guard = inner.lock().expect("core app state poisoned");
            guard.app_state.clone()
        };
        Self::emit_event_with_inner(inner, AppEvent::StateChanged(snapshot)).await;
    }

    fn append_recent_event(inner: &mut InnerState, event: CoreEvent) {
        inner.app_state.recent_events.push(event);
        if inner.app_state.recent_events.len() > MAX_RECENT_EVENTS {
            let drain_count = inner.app_state.recent_events.len() - MAX_RECENT_EVENTS;
            inner.app_state.recent_events.drain(0..drain_count);
        }
    }

    async fn apply_qr_login_progress(
        &self,
        _scene_id: String,
        progress: QrLoginProgress,
    ) -> Result<(), AppError> {
        let maybe_notify = match progress {
            QrLoginProgress::Pending => None,
            QrLoginProgress::Confirmed(session) => {
                self.deps
                    .session_store
                    .save_session(&session)
                    .await
                    .map_err(AppError::from)?;
                {
                    let mut inner = self.inner.lock().expect("core app state poisoned");
                    inner.app_state.auth_state = AuthState::LoggedIn {
                        user_id: session.user_id,
                    };
                }
                Some(AppNotification {
                    title: "登录成功".to_string(),
                    body: "会话已建立并保存。".to_string(),
                })
            }
            QrLoginProgress::Expired => {
                let mut inner = self.inner.lock().expect("core app state poisoned");
                inner.app_state.auth_state = AuthState::Failed {
                    reason: "二维码已过期".to_string(),
                };
                None
            }
            QrLoginProgress::Rejected => {
                let mut inner = self.inner.lock().expect("core app state poisoned");
                inner.app_state.auth_state = AuthState::Failed {
                    reason: "登录被拒绝".to_string(),
                };
                None
            }
        };

        if let Some(message) = maybe_notify {
            self.deps
                .notifier
                .notify(message.clone())
                .await
                .map_err(AppError::from)?;
            self.emit_event(AppEvent::Notification(message)).await;
        }
        self.emit_state_changed().await;
        Ok(())
    }

    async fn stop_monitor_engine(&self) {
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

    async fn start_background_monitor(&self) {
        let session = match self.deps.session_store.load_session().await {
            Ok(Some(s)) => s,
            _ => return,
        };
        let config = {
            let guard = self.inner.lock().expect("core app state poisoned");
            guard.config.clone()
        };
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
            let mut guard = self.inner.lock().expect("core app state poisoned");
            guard.app_state.last_error = Some(e.to_string());
            guard.app_state.monitor_running = false;
        }
    }
}

#[async_trait]
impl AppService for CoreAppService {
    async fn handle_command(&self, cmd: AppCommand) -> Result<(), AppError> {
        match cmd {
            AppCommand::LoadConfig => {
                let config = self
                    .deps
                    .config_store
                    .load_config()
                    .await
                    .map_err(AppError::from)?;
                {
                    let mut inner = self.inner.lock().expect("core app state poisoned");
                    inner.config = config;
                }
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

                {
                    let mut inner = self.inner.lock().expect("core app state poisoned");
                    match session {
                        Some(session) => {
                            inner.app_state.auth_state = AuthState::LoggedIn {
                                user_id: session.user_id,
                            };
                            inner.app_state.last_error = None;
                        }
                        None => {
                            inner.app_state.auth_state = AuthState::LoggedOut;
                        }
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

                self.deps
                    .session_store
                    .save_session(&refreshed)
                    .await
                    .map_err(AppError::from)?;

                {
                    let mut inner = self.inner.lock().expect("core app state poisoned");
                    inner.app_state.auth_state = AuthState::LoggedIn {
                        user_id: refreshed.user_id,
                    };
                    inner.app_state.last_error = None;
                }

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
                {
                    let mut inner = self.inner.lock().expect("core app state poisoned");
                    inner.app_state.auth_state = AuthState::WaitingQrScan {
                        scene_id: bootstrap.scene_id,
                        token: bootstrap.token,
                    };
                    inner.app_state.last_error = None;
                }
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
            AppCommand::WaitLogin {
                scene_id,
                timeout_secs,
            } => {
                let progress = self
                    .deps
                    .api
                    .wait_qr_login(&scene_id, timeout_secs)
                    .await
                    .map_err(AppError::from)?;
                self.apply_qr_login_progress(scene_id, progress).await
            }
            AppCommand::Logout => {
                self.stop_monitor_engine().await;
                self.deps
                    .session_store
                    .clear_session()
                    .await
                    .map_err(AppError::from)?;
                {
                    let mut inner = self.inner.lock().expect("core app state poisoned");
                    inner.app_state.auth_state = AuthState::LoggedOut;
                    inner.app_state.monitor_running = false;
                }
                self.emit_state_changed().await;
                Ok(())
            }
            AppCommand::StartMonitor => {
                {
                    let inner = self.inner.lock().expect("core app state poisoned");
                    if matches!(inner.app_state.auth_state, AuthState::LoggedOut) {
                        return Err(AppError::InvalidCommand(
                            "cannot start monitor before login".to_string(),
                        ));
                    }
                    if inner.app_state.monitor_running {
                        return Ok(());
                    }
                }

                {
                    let mut inner = self.inner.lock().expect("core app state poisoned");
                    inner.app_state.monitor_running = true;
                    inner.app_state.last_error = None;
                }
                self.start_background_monitor().await;
                self.emit_state_changed().await;
                Ok(())
            }
            AppCommand::StopMonitor => {
                self.stop_monitor_engine().await;
                {
                    let mut inner = self.inner.lock().expect("core app state poisoned");
                    inner.app_state.monitor_running = false;
                }
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
                {
                    let mut inner = self.inner.lock().expect("core app state poisoned");
                    inner.config = config;
                }
                self.emit_state_changed().await;
                Ok(())
            }
        }
    }

    async fn handle_query(&self, query: AppQuery) -> Result<AppQueryResult, AppError> {
        let inner = self.inner.lock().expect("core app state poisoned");
        match query {
            AppQuery::GetAppState => Ok(AppQueryResult::State(inner.app_state.clone())),
            AppQuery::GetConfig => Ok(AppQueryResult::Config(inner.config.clone())),
            AppQuery::GetRecentEvents { limit } => {
                let mut events = inner.app_state.recent_events.clone();
                if events.len() > limit {
                    events = events.split_off(events.len() - limit);
                }
                Ok(AppQueryResult::Events(events))
            }
        }
    }

    fn subscribe_events(&self) -> mpsc::Receiver<AppEvent> {
        let (tx, rx) = mpsc::channel(128);
        let mut guard = self.inner.lock().expect("core app state poisoned");
        guard.subscribers.push(tx);
        rx
    }
}

#[cfg(test)]
mod tests {
    use std::num::NonZeroU64;
    use std::sync::{Arc, Mutex};

    use async_trait::async_trait;
    use chrono::Utc;
    use tokio::sync::mpsc;
    use tokio::time::{Duration, sleep};

    use crate::app::ports::{
        ApiPort, ApiPortError, ConfigStorePort, LessonWsEvent, NotifierPort, NotifyPortError,
        SessionStorePort, StoragePortError, UpdateCheckerPort, UpdateInfo, UpdatePortError,
    };
    use crate::app::{
        AppCommand, AppConfigDto, AppError, AppNotification, AppQuery, AppQueryResult,
    };
    use crate::auth::{AuthSession, QrLoginBootstrap, QrLoginProgress};
    use crate::domain::{
        AnswerPayload, CheckinId, CourseId, Lesson, LessonId, LessonStatus, Problem, ProblemId,
        ProblemOption, ProblemType,
    };

    use super::{AppEvent, AppService};
    use super::{CoreAppDeps, CoreAppService};

    #[derive(Clone)]
    struct MockPorts {
        config: Arc<Mutex<AppConfigDto>>,
        session: Arc<Mutex<Option<AuthSession>>>,
        lessons: Arc<Mutex<Vec<Lesson>>>,
        problems: Arc<Mutex<Vec<Problem>>>,
        notifications: Arc<Mutex<Vec<AppNotification>>>,
        update: Arc<Mutex<Option<UpdateInfo>>>,
    }

    impl MockPorts {
        fn new(config: AppConfigDto) -> Self {
            Self {
                config: Arc::new(Mutex::new(config)),
                session: Arc::new(Mutex::new(None)),
                lessons: Arc::new(Mutex::new(Vec::new())),
                problems: Arc::new(Mutex::new(Vec::new())),
                notifications: Arc::new(Mutex::new(Vec::new())),
                update: Arc::new(Mutex::new(None)),
            }
        }
    }

    #[async_trait]
    impl ApiPort for MockPorts {
        async fn get_on_lessons(
            &self,
            _session: &AuthSession,
        ) -> Result<Vec<Lesson>, ApiPortError> {
            Ok(self.lessons.lock().expect("lessons poisoned").clone())
        }

        async fn get_lesson_problems(
            &self,
            _session: &AuthSession,
            lesson_id: LessonId,
        ) -> Result<Vec<Problem>, ApiPortError> {
            let problems = self.problems.lock().expect("problems poisoned").clone();
            Ok(problems
                .into_iter()
                .filter(|problem| problem.lesson_id == lesson_id)
                .collect())
        }

        async fn submit_answer(
            &self,
            _session: &AuthSession,
            _lesson_id: LessonId,
            _problem_id: ProblemId,
            _payload: AnswerPayload,
        ) -> Result<(), ApiPortError> {
            Ok(())
        }

        async fn submit_checkin(
            &self,
            _session: &AuthSession,
            _lesson_id: LessonId,
            _checkin_id: CheckinId,
        ) -> Result<(), ApiPortError> {
            Ok(())
        }

        async fn send_danmu(
            &self,
            _session: &AuthSession,
            _lesson_id: LessonId,
            _content: &str,
        ) -> Result<(), ApiPortError> {
            Ok(())
        }

        async fn start_qr_login(&self) -> Result<QrLoginBootstrap, ApiPortError> {
            Ok(QrLoginBootstrap {
                scene_id: "scene-1".to_string(),
                token: "token-1".to_string(),
                qr_svg: "<svg/>".to_string(),
            })
        }

        async fn poll_qr_login(&self, _scene_id: &str) -> Result<QrLoginProgress, ApiPortError> {
            Ok(QrLoginProgress::Confirmed(AuthSession {
                user_id: 42,
                access_token: "access-token".to_string(),
                refresh_token: Some("refresh-token".to_string()),
                expires_at_unix_ms: None,
            }))
        }

        async fn wait_qr_login(
            &self,
            _scene_id: &str,
            _timeout_secs: u64,
        ) -> Result<QrLoginProgress, ApiPortError> {
            self.poll_qr_login(_scene_id).await
        }

        async fn refresh_session(&self, _refresh_token: &str) -> Result<AuthSession, ApiPortError> {
            let refreshed_access = format!("refreshed-{}", _refresh_token);
            Ok(AuthSession {
                user_id: 42,
                access_token: refreshed_access,
                refresh_token: Some(_refresh_token.to_string()),
                expires_at_unix_ms: None,
            })
        }

        async fn connect_lesson_stream(
            &self,
            _session: &AuthSession,
            lesson_id: LessonId,
        ) -> Result<mpsc::Receiver<LessonWsEvent>, ApiPortError> {
            let problems = self
                .problems
                .lock()
                .expect("problems poisoned")
                .iter()
                .filter(|problem| problem.lesson_id == lesson_id)
                .cloned()
                .collect::<Vec<_>>();

            let (tx, rx) = mpsc::channel(32);
            tokio::spawn(async move {
                for problem in problems {
                    if tx
                        .send(LessonWsEvent::ProblemPublished { problem })
                        .await
                        .is_err()
                    {
                        break;
                    }
                }
            });
            Ok(rx)
        }
    }

    #[async_trait]
    impl SessionStorePort for MockPorts {
        async fn load_session(&self) -> Result<Option<AuthSession>, StoragePortError> {
            Ok(self.session.lock().expect("session poisoned").clone())
        }

        async fn save_session(&self, session: &AuthSession) -> Result<(), StoragePortError> {
            *self.session.lock().expect("session poisoned") = Some(session.clone());
            Ok(())
        }

        async fn clear_session(&self) -> Result<(), StoragePortError> {
            *self.session.lock().expect("session poisoned") = None;
            Ok(())
        }
    }

    #[async_trait]
    impl ConfigStorePort for MockPorts {
        async fn load_config(&self) -> Result<AppConfigDto, StoragePortError> {
            Ok(self.config.lock().expect("config poisoned").clone())
        }

        async fn save_config(&self, config: &AppConfigDto) -> Result<(), StoragePortError> {
            *self.config.lock().expect("config poisoned") = config.clone();
            Ok(())
        }
    }

    #[async_trait]
    impl NotifierPort for MockPorts {
        async fn notify(&self, message: AppNotification) -> Result<(), NotifyPortError> {
            self.notifications
                .lock()
                .expect("notifications poisoned")
                .push(message);
            Ok(())
        }
    }

    #[async_trait]
    impl UpdateCheckerPort for MockPorts {
        async fn check_latest(
            &self,
            _current_version: &str,
        ) -> Result<Option<UpdateInfo>, UpdatePortError> {
            Ok(self.update.lock().expect("update poisoned").clone())
        }
    }

    fn default_config() -> AppConfigDto {
        AppConfigDto::default()
    }

    #[tokio::test]
    async fn login_flow_should_update_state_to_logged_in() {
        let ports = Arc::new(MockPorts::new(default_config()));
        let app = CoreAppService::new(
            CoreAppDeps {
                api: ports.clone(),
                config_store: ports.clone(),
                session_store: ports.clone(),
                notifier: ports.clone(),
                update_checker: ports.clone(),
                monitor_engine: Arc::new(crate::monitor::CoreMonitorEngine::new(ports.clone())),
            },
            default_config(),
        );

        app.handle_command(AppCommand::LoginByQr)
            .await
            .expect("login bootstrap failed");
        app.handle_command(AppCommand::PollLogin {
            scene_id: "scene-1".to_string(),
        })
        .await
        .expect("poll login failed");

        let state = app
            .handle_query(AppQuery::GetAppState)
            .await
            .expect("query state failed");
        let AppQueryResult::State(state) = state else {
            panic!("expected state query result");
        };

        assert!(matches!(
            state.auth_state,
            crate::auth::AuthState::LoggedIn { user_id: 42 }
        ));
    }

    #[tokio::test]
    async fn restore_session_should_recover_logged_in_state() {
        let ports = Arc::new(MockPorts::new(default_config()));
        *ports.session.lock().expect("session poisoned") = Some(AuthSession {
            user_id: 77,
            access_token: "restored-access-token".to_string(),
            refresh_token: Some("restored-refresh-token".to_string()),
            expires_at_unix_ms: None,
        });

        let app = CoreAppService::new(
            CoreAppDeps {
                api: ports.clone(),
                config_store: ports.clone(),
                session_store: ports.clone(),
                notifier: ports.clone(),
                update_checker: ports.clone(),
                monitor_engine: Arc::new(crate::monitor::CoreMonitorEngine::new(ports.clone())),
            },
            default_config(),
        );

        app.handle_command(AppCommand::RestoreSession)
            .await
            .expect("restore session failed");

        let state = app
            .handle_query(AppQuery::GetAppState)
            .await
            .expect("query state failed");
        let AppQueryResult::State(state) = state else {
            panic!("expected state query result");
        };

        assert!(matches!(
            state.auth_state,
            crate::auth::AuthState::LoggedIn { user_id: 77 }
        ));
    }

    #[tokio::test]
    async fn load_config_should_sync_runtime_config_from_store() {
        let ports = Arc::new(MockPorts::new(default_config()));
        *ports.config.lock().expect("config poisoned") = AppConfigDto {
            monitor_interval_secs: 9,
            auto_checkin_enabled: false,
            auto_answer_enabled: true,
            auto_answer_random_guess: false,
            auto_danmu_enabled: true,
            danmu_threshold: 4,
            answer_delay_ms: 1200,
            answer_delay_type: 2,
            answer_delay_custom_percent: 30,
            notify_enabled: false,
            webhook_url: "http://example.com/webhook".to_string(),
            check_update_on_startup: false,
            tenant: "Rain".to_string(),
            auth_state_hint: Some(crate::auth::AuthState::Failed {
                reason: "loaded".to_string(),
            }),
        };

        let app = CoreAppService::new(
            CoreAppDeps {
                api: ports.clone(),
                config_store: ports.clone(),
                session_store: ports.clone(),
                notifier: ports.clone(),
                update_checker: ports.clone(),
                monitor_engine: Arc::new(crate::monitor::CoreMonitorEngine::new(ports.clone())),
            },
            default_config(),
        );

        app.handle_command(AppCommand::LoadConfig)
            .await
            .expect("load config failed");

        let config = app
            .handle_query(AppQuery::GetConfig)
            .await
            .expect("query config failed");
        let AppQueryResult::Config(config) = config else {
            panic!("expected config query result");
        };

        assert_eq!(config.monitor_interval_secs, 9);
        assert!(!config.auto_checkin_enabled);
        assert_eq!(config.answer_delay_ms, 1200);
        assert!(!config.notify_enabled);
        assert!(!config.check_update_on_startup);
        assert!(matches!(
            config.auth_state_hint,
            Some(crate::auth::AuthState::Failed { reason }) if reason == "loaded"
        ));
    }

    #[tokio::test]
    async fn refresh_session_should_update_saved_session_token() {
        let ports = Arc::new(MockPorts::new(default_config()));
        *ports.session.lock().expect("session poisoned") = Some(AuthSession {
            user_id: 42,
            access_token: "old-access-token".to_string(),
            refresh_token: Some("refresh-token".to_string()),
            expires_at_unix_ms: None,
        });

        let app = CoreAppService::new(
            CoreAppDeps {
                api: ports.clone(),
                config_store: ports.clone(),
                session_store: ports.clone(),
                notifier: ports.clone(),
                update_checker: ports.clone(),
                monitor_engine: Arc::new(crate::monitor::CoreMonitorEngine::new(ports.clone())),
            },
            default_config(),
        );

        app.handle_command(AppCommand::RefreshSession)
            .await
            .expect("refresh session failed");

        let saved = ports
            .session
            .lock()
            .expect("session poisoned")
            .clone()
            .expect("session missing after refresh");
        assert_eq!(saved.access_token, "refreshed-refresh-token");
    }

    #[tokio::test]
    async fn refresh_session_should_fallback_to_access_token_when_refresh_missing() {
        let ports = Arc::new(MockPorts::new(default_config()));
        *ports.session.lock().expect("session poisoned") = Some(AuthSession {
            user_id: 42,
            access_token: "access-only-token".to_string(),
            refresh_token: None,
            expires_at_unix_ms: None,
        });

        let app = CoreAppService::new(
            CoreAppDeps {
                api: ports.clone(),
                config_store: ports.clone(),
                session_store: ports.clone(),
                notifier: ports.clone(),
                update_checker: ports.clone(),
                monitor_engine: Arc::new(crate::monitor::CoreMonitorEngine::new(ports.clone())),
            },
            default_config(),
        );

        app.handle_command(AppCommand::RefreshSession)
            .await
            .expect("refresh session failed");

        let saved = ports
            .session
            .lock()
            .expect("session poisoned")
            .clone()
            .expect("session missing after refresh");
        assert_eq!(saved.access_token, "refreshed-access-only-token");
    }

    #[tokio::test]
    async fn start_monitor_should_fail_when_logged_out() {
        let ports = Arc::new(MockPorts::new(default_config()));
        let app = CoreAppService::new(
            CoreAppDeps {
                api: ports.clone(),
                config_store: ports.clone(),
                session_store: ports.clone(),
                notifier: ports.clone(),
                update_checker: ports.clone(),
                monitor_engine: Arc::new(crate::monitor::CoreMonitorEngine::new(ports.clone())),
            },
            default_config(),
        );

        let result = app.handle_command(AppCommand::StartMonitor).await;
        assert!(matches!(result, Err(AppError::InvalidCommand(_))));
    }

    #[tokio::test]
    async fn check_update_should_emit_update_available_event() {
        let ports = Arc::new(MockPorts::new(default_config()));
        *ports.update.lock().expect("update poisoned") = Some(UpdateInfo {
            latest_version: "9.9.9".to_string(),
            release_url: "https://example.com/release".to_string(),
            published_at_unix_ms: 0,
        });

        let app = CoreAppService::new(
            CoreAppDeps {
                api: ports.clone(),
                config_store: ports.clone(),
                session_store: ports.clone(),
                notifier: ports.clone(),
                update_checker: ports.clone(),
                monitor_engine: Arc::new(crate::monitor::CoreMonitorEngine::new(ports.clone())),
            },
            default_config(),
        );

        let mut events = app.subscribe_events();
        app.handle_command(AppCommand::CheckUpdate)
            .await
            .expect("check update failed");

        let event = events.recv().await.expect("expected one app event");
        assert!(matches!(event, AppEvent::UpdateAvailable { .. }));
    }

    #[tokio::test]
    async fn start_and_stop_monitor_should_toggle_running_state() {
        let ports = Arc::new(MockPorts::new(default_config()));
        let app = CoreAppService::new(
            CoreAppDeps {
                api: ports.clone(),
                config_store: ports.clone(),
                session_store: ports.clone(),
                notifier: ports.clone(),
                update_checker: ports.clone(),
                monitor_engine: Arc::new(crate::monitor::CoreMonitorEngine::new(ports.clone())),
            },
            default_config(),
        );

        app.handle_command(AppCommand::LoginByQr)
            .await
            .expect("login bootstrap failed");
        app.handle_command(AppCommand::PollLogin {
            scene_id: "scene-1".to_string(),
        })
        .await
        .expect("poll login failed");

        app.handle_command(AppCommand::StartMonitor)
            .await
            .expect("start monitor failed");

        let state = app
            .handle_query(AppQuery::GetAppState)
            .await
            .expect("query state failed");
        let AppQueryResult::State(state) = state else {
            panic!("expected state query result");
        };
        assert!(state.monitor_running);

        app.handle_command(AppCommand::StopMonitor)
            .await
            .expect("stop monitor failed");

        let state = app
            .handle_query(AppQuery::GetAppState)
            .await
            .expect("query state failed");
        let AppQueryResult::State(state) = state else {
            panic!("expected state query result");
        };
        assert!(!state.monitor_running);
    }

    #[tokio::test]
    async fn monitor_should_emit_auto_answer_event_when_problem_available() {
        let mut config = default_config();
        config.auto_answer_enabled = true;
        config.monitor_interval_secs = 1; // Faster poll for tests
        let ports = Arc::new(MockPorts::new(config.clone()));
        ports
            .lessons
            .lock()
            .expect("lessons poisoned")
            .push(Lesson {
                lesson_id: LessonId(NonZeroU64::new(1001).expect("non-zero")),
                course_id: CourseId(NonZeroU64::new(2001).expect("non-zero")),
                course_name: "测试课程".to_string(),
                teacher_name: "测试老师".to_string(),
                started_at: None,
                ended_at: None,
                status: LessonStatus::Running,
            });
        ports
            .problems
            .lock()
            .expect("problems poisoned")
            .push(Problem {
                lesson_id: LessonId(NonZeroU64::new(1001).expect("non-zero")),
                problem_id: ProblemId(NonZeroU64::new(3001).expect("non-zero")),
                problem_type: ProblemType::SingleChoice,
                title: "测试题目".to_string(),
                options: vec![ProblemOption {
                    option_id: "A".to_string(),
                    text: "选项A".to_string(),
                }],
                correct_answers: vec!["A".to_string()],
                blanks: Vec::new(),
                limit_secs: Some(10), // Bypass delay logic
                published_at: Utc::now(),
                deadline_at: None,
            });

        let mut config = default_config();
        config.monitor_interval_secs = 1; // Faster poll for tests
        let app = CoreAppService::new(
            CoreAppDeps {
                api: ports.clone(),
                config_store: ports.clone(),
                session_store: ports.clone(),
                notifier: ports.clone(),
                update_checker: ports.clone(),
                monitor_engine: Arc::new(crate::monitor::CoreMonitorEngine::new(ports.clone())),
            },
            config,
        );

        app.handle_command(AppCommand::LoginByQr)
            .await
            .expect("login bootstrap failed");
        app.handle_command(AppCommand::PollLogin {
            scene_id: "scene-1".to_string(),
        })
        .await
        .expect("poll login failed");

        app.handle_command(AppCommand::StartMonitor)
            .await
            .expect("start monitor failed");

        sleep(Duration::from_millis(1500)).await;

        let events = app
            .handle_query(AppQuery::GetRecentEvents { limit: 20 })
            .await
            .expect("query events failed");
        let AppQueryResult::Events(events) = events else {
            panic!("expected events query result");
        };

        assert!(events.iter().any(|event| {
            matches!(
                event,
                crate::monitor::CoreEvent::AutoAnswerSubmitted {
                    lesson_id,
                    problem_id,
                } if lesson_id.0.get() == 1001 && problem_id.0.get() == 3001
            )
        }));

        app.handle_command(AppCommand::StopMonitor)
            .await
            .expect("stop monitor failed");
    }
}
