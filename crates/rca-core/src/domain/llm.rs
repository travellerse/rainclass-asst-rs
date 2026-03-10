use async_trait::async_trait;

use crate::domain::{AnswerPayload, Problem};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LlmConfig {
    pub enabled: bool,
    pub api_key: String,
    pub api_base: String,
    pub model_name: String,
}

impl Default for LlmConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            api_key: String::new(),
            api_base: "https://api.openai.com/v1".to_string(),
            model_name: "gpt-4o-mini".to_string(),
        }
    }
}

#[async_trait]
pub trait LlmService: Send + Sync {
    /// Request the LLM to provide an answer for the given problem using the specified config.
    async fn answer_problem(
        &self,
        problem: &Problem,
        config: &LlmConfig,
    ) -> Result<AnswerPayload, crate::domain::DomainError>;
}
