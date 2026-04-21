use serde_json::Value;

use rca_core::domain::{BlankAnswer, ProblemOption, ProblemType};

pub fn map_problem_type(raw: &Value) -> ProblemType {
    if let Some(code) = raw.as_i64() {
        return match code {
            1 => ProblemType::SingleChoice,
            2 => ProblemType::MultipleChoice,
            3 => ProblemType::FillBlank,
            _ => ProblemType::Unknown,
        };
    }

    let text = raw.as_str().unwrap_or_default().to_ascii_lowercase();
    if text.contains("multiple") {
        ProblemType::MultipleChoice
    } else if text.contains("single") || text.contains("choice") {
        ProblemType::SingleChoice
    } else if text.contains("blank") || text.contains("fill") {
        ProblemType::FillBlank
    } else {
        ProblemType::Unknown
    }
}

pub fn parse_problem_options(problem: &Value) -> Vec<ProblemOption> {
    let mut options = Vec::new();
    let candidates = problem
        .get("options")
        .or_else(|| problem.get("choices"))
        .or_else(|| problem.get("optionList"))
        .or_else(|| problem.get("choiceList"));

    if let Some(Value::Array(items)) = candidates {
        for (index, item) in items.iter().enumerate() {
            match item {
                Value::Object(_) => {
                    let option_id = item
                        .get("optionId")
                        .or_else(|| item.get("option_id"))
                        .or_else(|| item.get("id"))
                        .or_else(|| item.get("key"))
                        .and_then(Value::as_str)
                        .map(ToString::to_string)
                        .unwrap_or_else(|| index.to_string());
                    let text = item
                        .get("text")
                        .or_else(|| item.get("content"))
                        .or_else(|| item.get("label"))
                        .or_else(|| item.get("value"))
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_string();
                    options.push(ProblemOption { option_id, text });
                }
                Value::String(text) => {
                    options.push(ProblemOption {
                        option_id: index.to_string(),
                        text: text.clone(),
                    });
                }
                _ => {}
            }
        }
    }

    if options.is_empty()
        && let Some(Value::Array(answers)) = problem.get("answers")
    {
        for answer in answers {
            if let Some(text) = answer.as_str() {
                options.push(ProblemOption {
                    option_id: text.to_string(),
                    text: text.to_string(),
                });
            }
        }
    }

    options
}

pub fn parse_correct_answers(problem: &Value) -> Vec<String> {
    let mut answers = Vec::new();
    if let Some(Value::Array(items)) = problem.get("answers") {
        for item in items {
            match item {
                Value::String(s) => answers.push(s.clone()),
                Value::Number(n) => answers.push(n.to_string()),
                Value::Bool(b) => answers.push(b.to_string()),
                _ => {}
            }
        }
    }
    answers
}

pub fn parse_blanks(problem: &Value) -> Vec<BlankAnswer> {
    let mut blanks = Vec::new();
    if let Some(Value::Array(items)) = problem.get("blanks") {
        for item in items {
            let mut accepted = Vec::new();
            if let Some(Value::Array(answers)) = item.get("answers") {
                for answer in answers {
                    match answer {
                        Value::String(s) => accepted.push(s.clone()),
                        Value::Number(n) => accepted.push(n.to_string()),
                        _ => {}
                    }
                }
            }
            blanks.push(BlankAnswer {
                accepted_values: accepted,
            });
        }
    }
    blanks
}

pub fn parse_limit(problem: &Value) -> Option<i64> {
    problem
        .get("limit")
        .and_then(Value::as_i64)
        .filter(|&v| v != -1)
}
