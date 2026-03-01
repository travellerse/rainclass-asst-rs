use crate::domain::{AnswerDecision, DomainError, Problem};

pub trait AnswerPolicy: Send + Sync {
    fn decide(&self, problem: &Problem) -> Result<AnswerDecision, DomainError>;
}
