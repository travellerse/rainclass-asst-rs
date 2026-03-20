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

/// Notification event keys used by `AppConfigDto.notify_events`.
///
/// These keys are part of the persisted config surface (e.g. `config.json`).
/// Prefer using these constants instead of hard-coded strings to avoid typos
/// and accidental drift.
pub mod notify_event_keys {
    // Notes:
    // - These string keys are persisted in user config (e.g. `config.json`).
    // - Renaming an existing key is a breaking change for existing users.
    // - Adding a new key is backward-compatible when the default is `true`.

    /// Login succeeded (QR confirmed and session saved).
    pub const LOGIN_SUCCESS: &str = "login_success";
    /// Auto-answer submission completed.
    pub const AUTO_ANSWER_SUBMITTED: &str = "auto_answer_submitted";
    /// Auto-checkin submission completed.
    pub const AUTO_CHECKIN_SUBMITTED: &str = "auto_checkin_submitted";
    /// Teacher initiated roll-call and the session was paused.
    pub const CALL_PAUSED: &str = "call_paused";
}
