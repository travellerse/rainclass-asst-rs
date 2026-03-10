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

/// Represents one blank in a fill-blank problem.
/// Each blank may accept multiple correct values.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlankAnswer {
    pub accepted_values: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Problem {
    pub lesson_id: LessonId,
    pub problem_id: ProblemId,
    pub problem_type: ProblemType,
    pub title: String,
    pub options: Vec<ProblemOption>,
    /// Correct answer IDs/values extracted from presentation slides.
    pub correct_answers: Vec<String>,
    /// Fill-blank answer slots (each with multiple accepted values).
    pub blanks: Vec<BlankAnswer>,
    /// Time limit in seconds (-1 means unlimited, mapped to None).
    pub limit_secs: Option<i64>,
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
    Llm,
}
