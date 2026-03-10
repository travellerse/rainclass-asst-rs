use async_openai::{
    Client,
    config::OpenAIConfig,
    types::chat::{
        ChatCompletionRequestSystemMessageArgs, ChatCompletionRequestUserMessageArgs,
        CreateChatCompletionRequestArgs,
    },
};
use async_trait::async_trait;
use rca_core::domain::{AnswerPayload, DomainError, LlmConfig, LlmService, Problem, ProblemType};

#[derive(Default)]
pub struct OpenAiLlmService;

impl OpenAiLlmService {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl LlmService for OpenAiLlmService {
    async fn answer_problem(
        &self,
        problem: &Problem,
        config: &LlmConfig,
    ) -> Result<AnswerPayload, DomainError> {
        let openai_config = OpenAIConfig::new()
            .with_api_key(&config.api_key)
            .with_api_base(&config.api_base);

        let client = Client::with_config(openai_config);

        let system_prompt = "You are an assistant designed to answer multiple-choice and fill-in-the-blank questions. You must ONLY output a valid JSON object as your final response, with no markdown formatting and no other text. The JSON format should match EXACTLY what is requested.";

        let problem_type_desc = match problem.problem_type {
            ProblemType::SingleChoice => "单选题 (Single Choice)",
            ProblemType::MultipleChoice => "多选题 (Multiple Choice)",
            ProblemType::FillBlank => "填空题 (Fill in the Blanks)",
            ProblemType::Unknown => {
                return Err(DomainError::InvalidValue(
                    "Unknown problem type cannot be answered".into(),
                ));
            }
        };

        let mut user_prompt = format!(
            "Question Type: {}\nQuestion Title: {}\n",
            problem_type_desc, problem.title
        );

        if !problem.options.is_empty() {
            user_prompt.push_str("Options:\n");
            for opt in &problem.options {
                user_prompt.push_str(&format!("- ID: {}, Text: {}\n", opt.option_id, opt.text));
            }
        }

        user_prompt.push_str("\nReturn a JSON object:\n");
        match problem.problem_type {
            ProblemType::SingleChoice => {
                user_prompt.push_str(
                    r#"{ "payload_type": "single", "option_id": "the_id_of_correct_option" }"#,
                );
            }
            ProblemType::MultipleChoice => {
                user_prompt
                    .push_str(r#"{ "payload_type": "multiple", "option_ids": ["id1", "id2"] }"#);
            }
            ProblemType::FillBlank => {
                user_prompt
                    .push_str(r#"{ "payload_type": "fill_blank", "text": "value1,value2" }"#);
                user_prompt.push_str(
                    "\nFor fill blank, join multiple blank answers with a comma if there are multiple blanks.",
                );
            }
            _ => {}
        }

        let request = CreateChatCompletionRequestArgs::default()
            .model(&config.model_name)
            .messages([
                ChatCompletionRequestSystemMessageArgs::default()
                    .content(system_prompt)
                    .build()
                    .map_err(|e| DomainError::LlmFailed(e.to_string()))?
                    .into(),
                ChatCompletionRequestUserMessageArgs::default()
                    .content(user_prompt)
                    .build()
                    .map_err(|e| DomainError::LlmFailed(e.to_string()))?
                    .into(),
            ])
            .build()
            .map_err(|e| DomainError::LlmFailed(e.to_string()))?;

        let response = client
            .chat()
            .create(request)
            .await
            .map_err(|e| DomainError::LlmFailed(e.to_string()))?;

        let content = response
            .choices
            .first()
            .and_then(|c| c.message.content.clone())
            .ok_or_else(|| DomainError::LlmFailed("No content in response".into()))?;

        let content = content.trim();
        let content = if content.starts_with("```json") {
            content.strip_prefix("```json").unwrap_or(content)
        } else {
            content
        };
        let content = if content.ends_with("```") {
            content.strip_suffix("```").unwrap_or(content)
        } else {
            content
        };
        let content = content.trim();

        let parsed: serde_json::Value = serde_json::from_str(content)
            .map_err(|e| DomainError::LlmFailed(format!("Failed to parse JSON: {}", e)))?;

        let payload_type = parsed
            .get("payload_type")
            .and_then(|v| v.as_str())
            .unwrap_or("");

        match payload_type {
            "single" => {
                let option_id = parsed
                    .get("option_id")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| DomainError::LlmFailed("Missing option_id".into()))?
                    .to_string();
                Ok(AnswerPayload::Single { option_id })
            }
            "multiple" => {
                let option_ids_val = parsed
                    .get("option_ids")
                    .and_then(|v| v.as_array())
                    .ok_or_else(|| DomainError::LlmFailed("Missing option_ids".into()))?;
                let mut option_ids = Vec::new();
                for v in option_ids_val {
                    if let Some(s) = v.as_str() {
                        option_ids.push(s.to_string());
                    }
                }
                Ok(AnswerPayload::Multiple { option_ids })
            }
            "fill_blank" => {
                let text = parsed
                    .get("text")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| DomainError::LlmFailed("Missing text".into()))?
                    .to_string();
                Ok(AnswerPayload::FillBlank { text })
            }
            _ => Err(DomainError::LlmFailed(format!(
                "Unknown payload type: {}",
                payload_type
            ))),
        }
    }
}
