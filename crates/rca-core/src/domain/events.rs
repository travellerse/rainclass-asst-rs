use chrono::{DateTime, Utc};
use uuid::Uuid;

use crate::domain::{CheckinId, Lesson, LessonId, Problem};

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct TraceId(pub Uuid);

#[derive(Debug, Clone)]
pub struct EventMeta {
    pub trace_id: TraceId,
    pub occurred_at: DateTime<Utc>,
    pub source: &'static str,
}

#[derive(Debug, Clone)]
pub enum DomainEvent {
    LessonStarted {
        meta: EventMeta,
        lesson: Lesson,
    },
    LessonEnded {
        meta: EventMeta,
        lesson_id: LessonId,
    },
    ProblemPublished {
        meta: EventMeta,
        problem: Problem,
    },
    CheckinOpened {
        meta: EventMeta,
        lesson_id: LessonId,
        checkin_id: CheckinId,
    },
}
