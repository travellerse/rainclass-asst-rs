use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use tokio::sync::mpsc;

use rca_core::app::ports::{
    ApiPort, ApiPortError, ConfigStorePort, LessonWsEvent, NotifierPort, NotifyPortError,
    SessionStorePort, StoragePortError, UpdateCheckerPort, UpdateInfo, UpdatePortError,
};
use rca_core::app::{AppConfigDto, AppNotification};
use rca_core::auth::{AuthSession, QrLoginBootstrap, QrLoginProgress};
use rca_core::domain::{AnswerPayload, CheckinId, Lesson, LessonId, Problem, ProblemId};

#[derive(Debug, Clone)]
pub struct MockInfraState {
    pub config: AppConfigDto,
    pub session: Option<AuthSession>,
    pub lessons: Vec<Lesson>,
    pub problems: Vec<Problem>,
    pub notifications: Vec<AppNotification>,
    pub update: Option<UpdateInfo>,
}

#[derive(Clone)]
pub struct MockInfra {
    state: Arc<Mutex<MockInfraState>>,
}

impl MockInfra {
    pub fn new(config: AppConfigDto) -> Self {
        Self {
            state: Arc::new(Mutex::new(MockInfraState {
                config,
                session: None,
                lessons: Vec::new(),
                problems: Vec::new(),
                notifications: Vec::new(),
                update: None,
            })),
        }
    }

    pub fn state(&self) -> Arc<Mutex<MockInfraState>> {
        Arc::clone(&self.state)
    }

    pub fn set_update(&self, update: Option<UpdateInfo>) {
        let mut guard = self.state.lock().expect("mock infra state poisoned");
        guard.update = update;
    }
}

#[async_trait]
impl ApiPort for MockInfra {
    async fn get_on_lessons(&self, _session: &AuthSession) -> Result<Vec<Lesson>, ApiPortError> {
        let guard = self.state.lock().expect("mock infra state poisoned");
        Ok(guard.lessons.clone())
    }

    async fn get_lesson_problems(
        &self,
        _session: &AuthSession,
        lesson_id: LessonId,
    ) -> Result<Vec<Problem>, ApiPortError> {
        let guard = self.state.lock().expect("mock infra state poisoned");
        Ok(guard
            .problems
            .iter()
            .filter(|problem| problem.lesson_id == lesson_id)
            .cloned()
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

    async fn start_qr_login(&self) -> Result<QrLoginBootstrap, ApiPortError> {
        Ok(QrLoginBootstrap {
            scene_id: "mock-scene".to_string(),
            token: "mock-token".to_string(),
            qr_svg: "<svg />".to_string(),
        })
    }

    async fn poll_qr_login(&self, _scene_id: &str) -> Result<QrLoginProgress, ApiPortError> {
        Ok(QrLoginProgress::Confirmed(AuthSession {
            user_id: 10001,
            access_token: "mock-access-token".to_string(),
            refresh_token: Some("mock-refresh-token".to_string()),
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

    async fn refresh_session(&self, refresh_token: &str) -> Result<AuthSession, ApiPortError> {
        Ok(AuthSession {
            user_id: 10001,
            access_token: format!("refreshed-{refresh_token}"),
            refresh_token: Some(refresh_token.to_string()),
            expires_at_unix_ms: None,
        })
    }

    async fn connect_lesson_stream(
        &self,
        _session: &AuthSession,
        lesson_id: LessonId,
    ) -> Result<mpsc::Receiver<LessonWsEvent>, ApiPortError> {
        let problems = {
            let guard = self.state.lock().expect("mock infra state poisoned");
            guard
                .problems
                .iter()
                .filter(|problem| problem.lesson_id == lesson_id)
                .cloned()
                .collect::<Vec<_>>()
        };

        let (tx, rx) = mpsc::channel(64);
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
impl SessionStorePort for MockInfra {
    async fn load_session(&self) -> Result<Option<AuthSession>, StoragePortError> {
        let guard = self.state.lock().expect("mock infra state poisoned");
        Ok(guard.session.clone())
    }

    async fn save_session(&self, session: &AuthSession) -> Result<(), StoragePortError> {
        let mut guard = self.state.lock().expect("mock infra state poisoned");
        guard.session = Some(session.clone());
        Ok(())
    }

    async fn clear_session(&self) -> Result<(), StoragePortError> {
        let mut guard = self.state.lock().expect("mock infra state poisoned");
        guard.session = None;
        Ok(())
    }
}

#[async_trait]
impl ConfigStorePort for MockInfra {
    async fn load_config(&self) -> Result<AppConfigDto, StoragePortError> {
        let guard = self.state.lock().expect("mock infra state poisoned");
        Ok(guard.config.clone())
    }

    async fn save_config(&self, config: &AppConfigDto) -> Result<(), StoragePortError> {
        let mut guard = self.state.lock().expect("mock infra state poisoned");
        guard.config = config.clone();
        Ok(())
    }
}

#[async_trait]
impl NotifierPort for MockInfra {
    async fn notify(&self, message: AppNotification) -> Result<(), NotifyPortError> {
        let mut guard = self.state.lock().expect("mock infra state poisoned");
        guard.notifications.push(message);
        Ok(())
    }
}

#[async_trait]
impl UpdateCheckerPort for MockInfra {
    async fn check_latest(
        &self,
        _current_version: &str,
    ) -> Result<Option<UpdateInfo>, UpdatePortError> {
        let guard = self.state.lock().expect("mock infra state poisoned");
        Ok(guard.update.clone())
    }
}
