use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::utils::count_tokens;

use super::{MAX_TOKENS, MESSAGE_EXTRA_TOKENS};

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Conversation {
    pub tokens: usize,
    pub messages: Vec<Message>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Message {
    pub role: MessageRole,
    pub content: String,
}

impl Conversation {
    pub fn new() -> Self {
        Self {
            tokens: 0,
            messages: vec![],
        }
    }

    pub fn add_chat(&mut self, input: &str, output: &str) -> Result<()> {
        self.messages.push(Message {
            role: MessageRole::User,
            content: input.to_string(),
        });
        self.messages.push(Message {
            role: MessageRole::Assistant,
            content: output.to_string(),
        });
        self.tokens += count_tokens(input) + count_tokens(output) + 2 * MESSAGE_EXTRA_TOKENS;
        Ok(())
    }

    /// Readline prompt
    pub fn add_prompt(&mut self, prompt: &str) {
        self.messages.push(Message {
            role: MessageRole::System,
            content: prompt.into(),
        });
        self.tokens += count_tokens(prompt) + MESSAGE_EXTRA_TOKENS;
    }

    pub fn echo_messages(&self, content: &str) -> String {
        let mut messages = self.messages.to_vec();
        messages.push(Message {
            role: MessageRole::User,
            content: content.into(),
        });
        serde_yaml::to_string(&messages).unwrap_or("Unable to echo message".into())
    }

    pub fn build_emssages(&self, content: &str) -> Value {
        let mut messages: Vec<Value> = self.messages.iter().map(msg_to_value).collect();
        messages.push(msg_to_value(&Message {
            role: MessageRole::User,
            content: content.into(),
        }));
        json!(messages)
    }

    pub fn reamind_tokens(&self) -> usize {
        MAX_TOKENS.saturating_sub(self.tokens)
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub enum MessageRole {
    System,
    Assistant,
    User,
}

impl MessageRole {
    pub fn name(&self) -> &'static str {
        match self {
            MessageRole::System => "system",
            MessageRole::Assistant => "assistant",
            MessageRole::User => "user",
        }
    }
}

fn msg_to_value(msg: &Message) -> Value {
    json!({ "role": msg.role.name(), "content": msg.content  })
}
