use chrono::{DateTime, Utc};

use crate::domain::{LessonId, ProblemId};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProblemType {
    SingleChoice,
    MultipleChoice,
    FillBlank,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProblemOption {
    pub option_id: String,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Problem {
    pub lesson_id: LessonId,
    pub problem_id: ProblemId,
    pub problem_type: ProblemType,
    pub title: String,
    pub options: Vec<ProblemOption>,
    pub published_at: DateTime<Utc>,
    pub deadline_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AnswerPayload {
    Single { option_id: String },
    Multiple { option_ids: Vec<String> },
    FillBlank { text: String },
}

#[derive(Debug, Clone, PartialEq)]
pub struct AnswerDecision {
    pub problem_id: ProblemId,
    pub payload: AnswerPayload,
    pub confidence: f32,
    pub source: AnswerSource,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AnswerSource {
    Heuristic,
    RuleBased,
    UserPreset,
}
