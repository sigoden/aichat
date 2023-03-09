use super::message::{num_tokens_from_messages, Message, MessageRole};
use super::role::Role;
use super::MAX_TOKENS;

use anyhow::Result;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Conversation {
    pub tokens: usize,
    pub role: Option<Role>,
    pub messages: Vec<Message>,
}

impl Conversation {
    pub fn new(role: Option<Role>) -> Self {
        let mut value = Self {
            tokens: 0,
            role,
            messages: vec![],
        };
        value.tokens = num_tokens_from_messages(&value.build_emssages(""));
        value
    }

    pub fn add_message(&mut self, input: &str, output: &str) -> Result<()> {
        let mut need_add_msg = true;
        if self.messages.is_empty() {
            if let Some(role) = self.role.as_ref() {
                self.messages.extend(role.build_emssages(input));
                need_add_msg = false;
            }
        }
        if need_add_msg {
            self.messages.push(Message {
                role: MessageRole::User,
                content: input.to_string(),
            });
        }
        self.messages.push(Message {
            role: MessageRole::Assistant,
            content: output.to_string(),
        });
        self.tokens = num_tokens_from_messages(&self.messages);
        Ok(())
    }

    pub fn echo_messages(&self, content: &str) -> String {
        let messages = self.build_emssages(content);
        serde_yaml::to_string(&messages).unwrap_or("Unable to echo message".into())
    }

    pub fn build_emssages(&self, content: &str) -> Vec<Message> {
        let mut messages = self.messages.to_vec();
        let mut need_add_msg = true;
        if messages.is_empty() {
            if let Some(role) = self.role.as_ref() {
                messages = role.build_emssages(content);
                need_add_msg = false;
            }
        };
        if need_add_msg {
            messages.push(Message {
                role: MessageRole::User,
                content: content.into(),
            });
        }
        messages
    }

    pub fn reamind_tokens(&self) -> usize {
        MAX_TOKENS.saturating_sub(self.tokens)
    }
}
