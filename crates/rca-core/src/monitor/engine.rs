use async_trait::async_trait;
use chrono::{DateTime, Utc};
use std::time::Duration;

use crate::auth::AuthSession;
use crate::domain::{CheckinId, Lesson, LessonId, Problem, ProblemId};
use crate::monitor::MonitorError;

#[derive(Debug, Clone)]
pub struct MonitorConfig {
    pub poll_interval: Duration,
    pub ws_reconnect_backoff_base: Duration,
    pub ws_reconnect_backoff_max: Duration,
    pub max_parallel_lessons: usize,
    pub auto_answer_enabled: bool,
    pub auto_checkin_enabled: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct MonitorTaskId(pub u64);

#[derive(Debug)]
pub struct MonitorHandle {
    pub task_id: MonitorTaskId,
}

#[derive(Debug, Clone)]
pub enum CoreEvent {
    MonitorStarted {
        at: DateTime<Utc>,
    },
    MonitorStopped {
        at: DateTime<Utc>,
    },
    LessonDiscovered {
        lesson: Lesson,
    },
    ProblemDiscovered {
        problem: Problem,
    },
    CheckinDiscovered {
        lesson_id: LessonId,
        checkin_id: CheckinId,
    },
    AutoAnswerSubmitted {
        lesson_id: LessonId,
        problem_id: ProblemId,
    },
    AutoCheckinSubmitted {
        lesson_id: LessonId,
        checkin_id: CheckinId,
    },
    DanmuPublished {
        lesson_id: LessonId,
        user_name: Option<String>,
        content: String,
    },
    CallPaused {
        lesson_id: LessonId,
        target_name: String,
    },
    PresentationUpdated {
        lesson_id: LessonId,
        presentation_id: u64,
    },
    Warning {
        code: &'static str,
        message: String,
    },
    Error {
        code: &'static str,
        message: String,
    },
}

#[async_trait]
pub trait MonitorEngine: Send + Sync {
    async fn start(
        &self,
        session: AuthSession,
        cfg: MonitorConfig,
    ) -> Result<MonitorHandle, MonitorError>;
    async fn stop(&self, handle: MonitorHandle) -> Result<(), MonitorError>;
    fn subscribe_events(&self) -> tokio::sync::broadcast::Receiver<CoreEvent>;
}
