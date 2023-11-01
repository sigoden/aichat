use crate::utils::count_tokens;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Message {
    pub role: MessageRole,
    pub content: String,
}

impl Message {
    pub fn new(content: &str) -> Self {
        Self {
            role: MessageRole::User,
            content: content.to_string(),
        }
    }
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MessageRole {
    System,
    Assistant,
    User,
}

impl MessageRole {
    #[allow(dead_code)]
    pub fn is_system(&self) -> bool {
        matches!(self, MessageRole::System)
    }
}

pub fn num_tokens_from_messages(messages: &[Message]) -> usize {
    let mut num_tokens = 0;
    for message in messages.iter() {
        num_tokens += 4;
        num_tokens += count_tokens(&message.content);
        num_tokens += 1; // role always take 1 token
    }
    num_tokens += 2;
    num_tokens
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_serde() {
        assert_eq!(
            serde_json::to_string(&Message::new("Hello World")).unwrap(),
            "{\"role\":\"user\",\"content\":\"Hello World\"}"
        );
    }
}
