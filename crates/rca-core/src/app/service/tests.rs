use std::num::NonZeroU64;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use chrono::Utc;
use tokio::sync::mpsc;
use tokio::sync::{Mutex as AsyncMutex, broadcast};
use tokio::time::{Duration, sleep};

use crate::app::ports::{
    ApiPort, ApiPortError, ConfigStorePort, LessonWsEvent, NotifierPort, NotifyPortError,
    SessionStorePort, StoragePortError, UpdateCheckerPort, UpdateInfo, UpdatePortError,
};
use crate::app::{AppCommand, AppConfigDto, AppError, AppNotification, AppQuery, AppQueryResult};
use crate::auth::{AuthSession, QrLoginBootstrap, QrLoginProgress};
use crate::domain::{
    AnswerPayload, CheckinId, CourseId, Lesson, LessonId, LessonStatus, Problem, ProblemId,
    ProblemOption, ProblemType,
};
use crate::monitor::{
    CoreEvent, MonitorConfig, MonitorEngine, MonitorError, MonitorHandle, MonitorTaskId,
};

use super::{AppServiceImpl, CoreAppDeps};
use crate::app::AppEvent;
use crate::app::AppService;

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

#[derive(Clone)]
struct RecordingMonitorEngine {
    start_task_id: u64,
    stopped_task_ids: Arc<AsyncMutex<Vec<u64>>>,
    event_tx: broadcast::Sender<CoreEvent>,
}

impl RecordingMonitorEngine {
    fn new(start_task_id: u64) -> Self {
        let (event_tx, _) = broadcast::channel(16);
        Self {
            start_task_id,
            stopped_task_ids: Arc::new(AsyncMutex::new(Vec::new())),
            event_tx,
        }
    }

    async fn stopped_task_ids(&self) -> Vec<u64> {
        self.stopped_task_ids.lock().await.clone()
    }
}

#[async_trait]
impl MonitorEngine for RecordingMonitorEngine {
    async fn start(
        &self,
        _session: AuthSession,
        _cfg: MonitorConfig,
    ) -> Result<MonitorHandle, MonitorError> {
        Ok(MonitorHandle {
            task_id: MonitorTaskId(self.start_task_id),
        })
    }

    async fn stop(&self, handle: MonitorHandle) -> Result<(), MonitorError> {
        self.stopped_task_ids.lock().await.push(handle.task_id.0);
        Ok(())
    }

    fn subscribe_events(&self) -> broadcast::Receiver<CoreEvent> {
        self.event_tx.subscribe()
    }
}

#[async_trait]
impl ApiPort for MockPorts {
    async fn get_on_lessons(&self, _session: &AuthSession) -> Result<Vec<Lesson>, ApiPortError> {
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

    async fn report_page_view(
        &self,
        _session: &AuthSession,
        _lesson: &Lesson,
        _slide_index: u64,
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
            csrf_token: Some("csrf-token".to_string()),
            original_id: Some("original-id".to_string()),
        }))
    }

    async fn wait_qr_login(
        &self,
        scene_id: &str,
        _timeout_secs: u64,
    ) -> Result<QrLoginProgress, ApiPortError> {
        self.poll_qr_login(scene_id).await
    }

    async fn refresh_session(&self, refresh_token: &str) -> Result<AuthSession, ApiPortError> {
        let refreshed_access = format!("refreshed-{refresh_token}");
        Ok(AuthSession {
            user_id: 42,
            access_token: refreshed_access,
            refresh_token: Some(refresh_token.to_string()),
            expires_at_unix_ms: None,
            csrf_token: None,
            original_id: None,
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

    async fn download_presentation(
        &self,
        _session: &AuthSession,
        _presentation_id: u64,
        _lesson_id: Option<u64>,
        _save_dir: &std::path::Path,
    ) -> Result<std::path::PathBuf, ApiPortError> {
        Ok(std::path::PathBuf::from("mock.pdf"))
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
    let app = AppServiceImpl::new_started(
        CoreAppDeps {
            api: ports.clone(),
            config_store: ports.clone(),
            session_store: ports.clone(),
            notifier: ports.clone(),
            update_checker: ports.clone(),
            monitor_engine: Arc::new(crate::monitor::CoreMonitorEngine::new(ports.clone())),
        },
        default_config(),
    )
    .await;

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
        csrf_token: Some("restored-csrf".to_string()),
        original_id: Some("restored-original".to_string()),
    });

    let app = AppServiceImpl::new_started(
        CoreAppDeps {
            api: ports.clone(),
            config_store: ports.clone(),
            session_store: ports.clone(),
            notifier: ports.clone(),
            update_checker: ports.clone(),
            monitor_engine: Arc::new(crate::monitor::CoreMonitorEngine::new(ports.clone())),
        },
        default_config(),
    )
    .await;

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
        notify_events: Default::default(),
        webhook_url: "http://example.com/webhook".to_string(),
        check_update_on_startup: false,
        tenant: crate::app::TenantKind::Rain,
        auth_state_hint: Some(crate::auth::AuthState::Failed {
            reason: "loaded".to_string(),
        }),
    };

    let app = AppServiceImpl::new_started(
        CoreAppDeps {
            api: ports.clone(),
            config_store: ports.clone(),
            session_store: ports.clone(),
            notifier: ports.clone(),
            update_checker: ports.clone(),
            monitor_engine: Arc::new(crate::monitor::CoreMonitorEngine::new(ports.clone())),
        },
        default_config(),
    )
    .await;

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
        csrf_token: Some("keep-csrf".to_string()),
        original_id: Some("keep-original".to_string()),
    });

    let app = AppServiceImpl::new_started(
        CoreAppDeps {
            api: ports.clone(),
            config_store: ports.clone(),
            session_store: ports.clone(),
            notifier: ports.clone(),
            update_checker: ports.clone(),
            monitor_engine: Arc::new(crate::monitor::CoreMonitorEngine::new(ports.clone())),
        },
        default_config(),
    )
    .await;

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
    assert_eq!(saved.csrf_token.as_deref(), Some("keep-csrf"));
    assert_eq!(saved.original_id.as_deref(), Some("keep-original"));
}

#[tokio::test]
async fn refresh_session_should_fallback_to_access_token_when_refresh_missing() {
    let ports = Arc::new(MockPorts::new(default_config()));
    *ports.session.lock().expect("session poisoned") = Some(AuthSession {
        user_id: 42,
        access_token: "access-only-token".to_string(),
        refresh_token: None,
        expires_at_unix_ms: None,
        csrf_token: Some("access-csrf".to_string()),
        original_id: Some("access-original".to_string()),
    });

    let app = AppServiceImpl::new_started(
        CoreAppDeps {
            api: ports.clone(),
            config_store: ports.clone(),
            session_store: ports.clone(),
            notifier: ports.clone(),
            update_checker: ports.clone(),
            monitor_engine: Arc::new(crate::monitor::CoreMonitorEngine::new(ports.clone())),
        },
        default_config(),
    )
    .await;

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
    assert_eq!(saved.csrf_token.as_deref(), Some("access-csrf"));
    assert_eq!(saved.original_id.as_deref(), Some("access-original"));
}

#[tokio::test]
async fn start_monitor_should_fail_when_logged_out() {
    let ports = Arc::new(MockPorts::new(default_config()));
    let app = AppServiceImpl::new_started(
        CoreAppDeps {
            api: ports.clone(),
            config_store: ports.clone(),
            session_store: ports.clone(),
            notifier: ports.clone(),
            update_checker: ports.clone(),
            monitor_engine: Arc::new(crate::monitor::CoreMonitorEngine::new(ports.clone())),
        },
        default_config(),
    )
    .await;

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

    let app = AppServiceImpl::new_started(
        CoreAppDeps {
            api: ports.clone(),
            config_store: ports.clone(),
            session_store: ports.clone(),
            notifier: ports.clone(),
            update_checker: ports.clone(),
            monitor_engine: Arc::new(crate::monitor::CoreMonitorEngine::new(ports.clone())),
        },
        default_config(),
    )
    .await;

    let mut events = app.subscribe_events().await;
    app.handle_command(AppCommand::CheckUpdate)
        .await
        .expect("check update failed");

    let event = events.recv().await.expect("expected one app event");
    assert!(matches!(event, AppEvent::UpdateAvailable { .. }));
}

#[tokio::test]
async fn start_and_stop_monitor_should_toggle_running_state() {
    let ports = Arc::new(MockPorts::new(default_config()));
    let app = AppServiceImpl::new_started(
        CoreAppDeps {
            api: ports.clone(),
            config_store: ports.clone(),
            session_store: ports.clone(),
            notifier: ports.clone(),
            update_checker: ports.clone(),
            monitor_engine: Arc::new(crate::monitor::CoreMonitorEngine::new(ports.clone())),
        },
        default_config(),
    )
    .await;

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
    config.monitor_interval_secs = 1;
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
            limit_secs: Some(10),
            published_at: Utc::now(),
            deadline_at: None,
        });
    let app = AppServiceImpl::new_started(
        CoreAppDeps {
            api: ports.clone(),
            config_store: ports.clone(),
            session_store: ports.clone(),
            notifier: ports.clone(),
            update_checker: ports.clone(),
            monitor_engine: Arc::new(crate::monitor::CoreMonitorEngine::new(ports.clone())),
        },
        config,
    )
    .await;

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

#[tokio::test]
async fn stop_monitor_should_use_handle_returned_by_start() {
    let ports = Arc::new(MockPorts::new(default_config()));
    let monitor_engine = Arc::new(RecordingMonitorEngine::new(77));
    let app = AppServiceImpl::new_started(
        CoreAppDeps {
            api: ports.clone(),
            config_store: ports.clone(),
            session_store: ports.clone(),
            notifier: ports.clone(),
            update_checker: ports.clone(),
            monitor_engine: monitor_engine.clone(),
        },
        default_config(),
    )
    .await;

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
    app.handle_command(AppCommand::StopMonitor)
        .await
        .expect("stop monitor failed");

    assert_eq!(monitor_engine.stopped_task_ids().await, vec![77]);
}

#[tokio::test]
async fn monitor_should_not_emit_auto_answer_event_when_disabled() {
    let mut config = default_config();
    config.auto_answer_enabled = false;
    config.monitor_interval_secs = 1;
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
            limit_secs: Some(10),
            published_at: Utc::now(),
            deadline_at: None,
        });

    let app = AppServiceImpl::new_started(
        CoreAppDeps {
            api: ports.clone(),
            config_store: ports.clone(),
            session_store: ports.clone(),
            notifier: ports.clone(),
            update_checker: ports.clone(),
            monitor_engine: Arc::new(crate::monitor::CoreMonitorEngine::new(ports.clone())),
        },
        config,
    )
    .await;

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

    assert!(!events.iter().any(|event| {
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

#[tokio::test]
async fn save_config_should_persist_and_update_runtime() {
    let ports = Arc::new(MockPorts::new(default_config()));
    let app = AppServiceImpl::new_started(
        CoreAppDeps {
            api: ports.clone(),
            config_store: ports.clone(),
            session_store: ports.clone(),
            notifier: ports.clone(),
            update_checker: ports.clone(),
            monitor_engine: Arc::new(crate::monitor::CoreMonitorEngine::new(ports.clone())),
        },
        default_config(),
    )
    .await;

    let mut new_config = default_config();
    new_config.monitor_interval_secs = 42;
    new_config.auto_checkin_enabled = false;

    app.handle_command(AppCommand::SaveConfig { config: new_config })
        .await
        .expect("save config failed");

    let result = app
        .handle_query(AppQuery::GetConfig)
        .await
        .expect("query config failed");
    let AppQueryResult::Config(cfg) = result else {
        panic!("expected config");
    };
    assert_eq!(cfg.monitor_interval_secs, 42);
    assert!(!cfg.auto_checkin_enabled);

    let stored = ports.config.lock().expect("config poisoned").clone();
    assert_eq!(stored.monitor_interval_secs, 42);
}

#[tokio::test]
async fn logout_should_clear_session_and_stop_monitor() {
    let ports = Arc::new(MockPorts::new(default_config()));
    let app = AppServiceImpl::new_started(
        CoreAppDeps {
            api: ports.clone(),
            config_store: ports.clone(),
            session_store: ports.clone(),
            notifier: ports.clone(),
            update_checker: ports.clone(),
            monitor_engine: Arc::new(crate::monitor::CoreMonitorEngine::new(ports.clone())),
        },
        default_config(),
    )
    .await;

    app.handle_command(AppCommand::LoginByQr)
        .await
        .expect("login bootstrap failed");
    app.handle_command(AppCommand::PollLogin {
        scene_id: "scene-1".to_string(),
    })
    .await
    .expect("poll login failed");

    app.handle_command(AppCommand::Logout)
        .await
        .expect("logout failed");

    let state = app
        .handle_query(AppQuery::GetAppState)
        .await
        .expect("query failed");
    let AppQueryResult::State(state) = state else {
        panic!("expected state");
    };
    assert!(matches!(
        state.auth_state,
        crate::auth::AuthState::LoggedOut
    ));
    assert!(!state.monitor_running);

    assert!(ports.session.lock().expect("session poisoned").is_none());
}

#[tokio::test]
async fn get_recent_events_should_respect_limit() {
    let ports = Arc::new(MockPorts::new(default_config()));
    let app = AppServiceImpl::new_started(
        CoreAppDeps {
            api: ports.clone(),
            config_store: ports.clone(),
            session_store: ports.clone(),
            notifier: ports.clone(),
            update_checker: ports.clone(),
            monitor_engine: Arc::new(crate::monitor::CoreMonitorEngine::new(ports.clone())),
        },
        default_config(),
    )
    .await;

    let result = app
        .handle_query(AppQuery::GetRecentEvents { limit: 0 })
        .await
        .expect("query events failed");
    let AppQueryResult::Events(events) = result else {
        panic!("expected events");
    };

    assert!(events.is_empty());
}

#[tokio::test]
async fn refresh_session_without_session_should_fail() {
    let ports = Arc::new(MockPorts::new(default_config()));
    let app = AppServiceImpl::new_started(
        CoreAppDeps {
            api: ports.clone(),
            config_store: ports.clone(),
            session_store: ports.clone(),
            notifier: ports.clone(),
            update_checker: ports.clone(),
            monitor_engine: Arc::new(crate::monitor::CoreMonitorEngine::new(ports.clone())),
        },
        default_config(),
    )
    .await;

    let result = app.handle_command(AppCommand::RefreshSession).await;
    assert!(matches!(result, Err(AppError::InvalidCommand(_))));
}

#[tokio::test]
async fn config_default_has_expected_values() {
    let cfg = AppConfigDto::default();
    assert_eq!(cfg.monitor_interval_secs, 5);
    assert!(cfg.auto_checkin_enabled);
    assert!(cfg.auto_answer_enabled);
    assert!(!cfg.auto_answer_random_guess);
    assert!(cfg.auto_danmu_enabled);
    assert_eq!(cfg.danmu_threshold, 4);
    assert_eq!(cfg.answer_delay_ms, 500);
    assert_eq!(cfg.answer_delay_type, 1);
    assert_eq!(cfg.answer_delay_custom_percent, 50);
    assert!(cfg.notify_enabled);
    assert!(cfg.webhook_url.is_empty());
    assert!(cfg.check_update_on_startup);
    assert_eq!(cfg.tenant, crate::app::TenantKind::Hetang);
    assert!(cfg.auth_state_hint.is_none());
}

#[tokio::test]
async fn start_monitor_when_already_running_is_noop() {
    let ports = Arc::new(MockPorts::new(default_config()));
    let app = AppServiceImpl::new_started(
        CoreAppDeps {
            api: ports.clone(),
            config_store: ports.clone(),
            session_store: ports.clone(),
            notifier: ports.clone(),
            update_checker: ports.clone(),
            monitor_engine: Arc::new(crate::monitor::CoreMonitorEngine::new(ports.clone())),
        },
        default_config(),
    )
    .await;

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
    app.handle_command(AppCommand::StartMonitor)
        .await
        .expect("second start monitor should succeed as no-op");

    let state = app
        .handle_query(AppQuery::GetAppState)
        .await
        .expect("query failed");
    let AppQueryResult::State(state) = state else {
        panic!("expected state");
    };
    assert!(state.monitor_running);

    app.handle_command(AppCommand::StopMonitor)
        .await
        .expect("stop monitor failed");
}

#[tokio::test]
async fn check_update_no_update_should_not_emit_event() {
    let ports = Arc::new(MockPorts::new(default_config()));
    let app = AppServiceImpl::new_started(
        CoreAppDeps {
            api: ports.clone(),
            config_store: ports.clone(),
            session_store: ports.clone(),
            notifier: ports.clone(),
            update_checker: ports.clone(),
            monitor_engine: Arc::new(crate::monitor::CoreMonitorEngine::new(ports.clone())),
        },
        default_config(),
    )
    .await;

    let mut events = app.subscribe_events().await;
    app.handle_command(AppCommand::CheckUpdate)
        .await
        .expect("check update failed");

    let result = tokio::time::timeout(Duration::from_millis(100), events.recv()).await;
    assert!(result.is_err(), "should not have received any event");
}
