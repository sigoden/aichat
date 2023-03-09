use super::message::{Message, MessageRole, MESSAGE_EXTRA_TOKENS};

use crate::utils::count_tokens;

use serde::{Deserialize, Serialize};

const TEMP_NAME: &str = "Ｐ";
const INPUT_PLACEHOLDER: &str = "__INPUT__";
const INPUT_PLACEHOLDER_TOKENS: usize = 3;

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Role {
    /// Role name
    pub name: String,
    /// Prompt text send to ai for setting up a role.
    ///
    /// If prmopt contains __INPUT___, it's embeded prompt
    /// If prmopt don't contain __INPUT___, it's system prompt
    pub prompt: String,
    /// What sampling temperature to use, between 0 and 2
    pub temperature: Option<f64>,
    /// Number of tokens
    ///
    /// System prompt consume extra 6 tokens
    #[serde(skip_deserializing)]
    pub tokens: usize,
}

impl Role {
    pub fn new(prompt: &str, temperature: Option<f64>) -> Self {
        let mut value = Self {
            name: TEMP_NAME.into(),
            prompt: prompt.into(),
            temperature,
            tokens: 0,
        };
        value.tokens = value.consume_tokens();
        value
    }

    pub fn is_temp(&self) -> bool {
        self.name == TEMP_NAME
    }

    pub fn consume_tokens(&self) -> usize {
        if self.embeded() {
            count_tokens(&self.prompt) + MESSAGE_EXTRA_TOKENS - INPUT_PLACEHOLDER_TOKENS
        } else {
            count_tokens(&self.prompt) + 2 * MESSAGE_EXTRA_TOKENS
        }
    }

    pub fn embeded(&self) -> bool {
        self.prompt.contains(INPUT_PLACEHOLDER)
    }

    pub fn echo_messages(&self, content: &str) -> String {
        if self.embeded() {
            merge_prompt_content(&self.prompt, content)
        } else {
            format!("{}{content}", self.prompt)
        }
    }

    pub fn build_emssages(&self, content: &str) -> Vec<Message> {
        if self.embeded() {
            let content = merge_prompt_content(&self.prompt, content);
            vec![Message {
                role: MessageRole::User,
                content,
            }]
        } else {
            vec![
                Message {
                    role: MessageRole::System,
                    content: self.prompt.clone(),
                },
                Message {
                    role: MessageRole::User,
                    content: content.to_string(),
                },
            ]
        }
    }
}

pub fn merge_prompt_content(prompt: &str, content: &str) -> String {
    prompt.replace(INPUT_PLACEHOLDER, content)
}
