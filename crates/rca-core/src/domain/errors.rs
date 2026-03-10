use thiserror::Error;

use crate::domain::{LessonStatus, ProblemId};

#[derive(Debug, Error)]
pub enum DomainError {
    #[error("invalid lesson status transition: {from:?} -> {to:?}")]
    InvalidLessonStatusTransition {
        from: LessonStatus,
        to: LessonStatus,
    },

    #[error("problem payload does not match problem type: {problem_id:?}")]
    ProblemPayloadMismatch { problem_id: ProblemId },

    #[error("invalid value: {0}")]
    InvalidValue(String),

    #[error("llm operation failed: {0}")]
    LlmFailed(String),
}
