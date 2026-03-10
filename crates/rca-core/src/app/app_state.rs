use crate::auth::AuthState;
use crate::domain::Lesson;
use crate::monitor::CoreEvent;

#[derive(Debug, Clone)]
pub struct AppState {
    pub auth_state: AuthState,
    pub monitor_running: bool,
    pub current_lessons: Vec<Lesson>,
    pub recent_events: Vec<CoreEvent>,
    pub last_error: Option<String>,
}

#[derive(Debug, Clone)]
pub enum AppEvent {
    StateChanged(AppState),
    Notification(AppNotification),
    UpdateAvailable {
        version: String,
        url: String,
    },
    PresentationDiscovered {
        lesson_id: crate::domain::LessonId,
        presentation_id: u64,
    },
}

#[derive(Debug, Clone)]
pub struct AppNotification {
    pub title: String,
    pub body: String,
}
