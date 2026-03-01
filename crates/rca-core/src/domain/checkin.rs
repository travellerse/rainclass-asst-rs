use chrono::{DateTime, Utc};

use crate::domain::{CheckinId, LessonId};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Checkin {
    pub lesson_id: LessonId,
    pub checkin_id: CheckinId,
    pub opened_at: DateTime<Utc>,
}
