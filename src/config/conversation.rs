use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Session {
    pub tokens: usize,
    pub messages: Vec<Message>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Message {
    pub role: MessageRole,
    pub content: String,
}

impl Session {
    pub fn new() -> Self {
        Self {
            tokens: 0,
            messages: vec![],
        }
    }

    pub fn add_conversatoin(&mut self, input: &str, output: &str) -> Result<()> {
        self.messages.push(Message {
            role: MessageRole::User,
            content: input.to_string(),
        });
        self.messages.push(Message {
            role: MessageRole::Assistant,
            content: output.to_string(),
        });
        Ok(())
    }

    /// Readline prompt
    pub fn add_prompt(&mut self, prompt: &str) {
        self.messages.push(Message {
            role: MessageRole::System,
            content: prompt.into(),
        });
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
