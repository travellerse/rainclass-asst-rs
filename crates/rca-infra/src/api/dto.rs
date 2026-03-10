use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OnLessonDto {
    pub lesson_id: u64,
    pub course_id: u64,
    pub course_name: String,
    pub teacher_name: String,
    pub status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProblemDto {
    pub lesson_id: u64,
    pub problem_id: u64,
    pub problem_type: String,
    pub title: String,
    pub options: Vec<(String, String)>,
    pub correct_answers: Vec<String>,
    pub blanks: Vec<Vec<String>>,
    pub limit_secs: Option<i64>,
    pub published_at: DateTime<Utc>,
    pub deadline_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CheckinDto {
    pub lesson_id: u64,
    pub checkin_id: u64,
    pub opened_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DanmuDto {
    pub lesson_id: u64,
    pub user_id: String,
    pub user_name: Option<String>,
    pub content: String,
    pub published_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CallPausedDto {
    pub lesson_id: u64,
    pub target_name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PresentationUpdatedDto {
    pub lesson_id: u64,
    pub presentation_id: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SlideNavigatedDto {
    pub lesson_id: u64,
    pub presentation_id: u64,
    pub slide_id: u64,
    pub slide_index: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProblemUnlockedDto {
    pub lesson_id: u64,
    pub problem_id: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum WsEventDto {
    ProblemPublished(ProblemDto),
    CheckinOpened(CheckinDto),
    DanmuPublished(DanmuDto),
    CallPaused(CallPausedDto),
    PresentationUpdated(PresentationUpdatedDto),
    SlideNavigated(SlideNavigatedDto),
    ProblemUnlocked(ProblemUnlockedDto),
    LessonEnded {
        lesson_id: u64,
    },
    Unknown {
        raw_type: String,
        raw_payload: String,
    },
}
