use chrono::{DateTime, Utc};

use crate::domain::{CourseId, LessonId};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LessonStatus {
    Scheduled,
    Running,
    Ended,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Lesson {
    pub lesson_id: LessonId,
    pub course_id: CourseId,
    pub course_name: String,
    pub teacher_name: String,
    pub started_at: Option<DateTime<Utc>>,
    pub ended_at: Option<DateTime<Utc>>,
    pub status: LessonStatus,
}
