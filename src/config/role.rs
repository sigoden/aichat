use super::message::{Message, MessageRole};

use serde::{Deserialize, Serialize};

const TEMP_NAME: &str = "Ｐ";
const INPUT_PLACEHOLDER: &str = "__INPUT__";

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
}

impl Role {
    pub fn new(prompt: &str, temperature: Option<f64>) -> Self {
        Self {
            name: TEMP_NAME.into(),
            prompt: prompt.into(),
            temperature,
        }
    }

    pub fn is_temp(&self) -> bool {
        self.name == TEMP_NAME
    }

    pub fn embeded(&self) -> bool {
        self.prompt.contains(INPUT_PLACEHOLDER)
    }

    pub fn echo_messages(&self, content: &str) -> String {
        if self.embeded() {
            merge_prompt_content(&self.prompt, content)
        } else {
            format!("{}\n{content}", self.prompt)
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
