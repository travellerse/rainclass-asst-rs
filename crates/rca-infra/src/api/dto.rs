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
pub enum WsEventDto {
    ProblemPublished(ProblemDto),
    CheckinOpened(CheckinDto),
    LessonEnded {
        lesson_id: u64,
    },
    Unknown {
        raw_type: String,
        raw_payload: String,
    },
}
