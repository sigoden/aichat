use super::Message;

use crate::utils::count_tokens;

use anyhow::{bail, Result};

#[derive(Debug, Clone)]
pub struct ModelInfo {
    pub client: String,
    pub name: String,
    pub index: usize,
    pub max_tokens: Option<usize>,
    pub per_message_tokens: usize,
    pub bias_tokens: usize,
}

impl Default for ModelInfo {
    fn default() -> Self {
        ModelInfo::new(0, "", "")
    }
}

impl ModelInfo {
    pub fn new(index: usize, client: &str, name: &str) -> Self {
        Self {
            index,
            client: client.into(),
            name: name.into(),
            max_tokens: None,
            per_message_tokens: 0,
            bias_tokens: 0,
        }
    }

    pub fn set_max_tokens(mut self, max_tokens: Option<usize>) -> Self {
        match max_tokens {
            None | Some(0) => self.max_tokens = None,
            _ => self.max_tokens = max_tokens,
        }
        self
    }

    pub fn set_tokens_formula(mut self, per_message_token: usize, bias_tokens: usize) -> Self {
        self.per_message_tokens = per_message_token;
        self.bias_tokens = bias_tokens;
        self
    }

    pub fn full_name(&self) -> String {
        format!("{}:{}", self.client, self.name)
    }

    pub fn messages_tokens(&self, messages: &[Message]) -> usize {
        messages.iter().map(|v| count_tokens(&v.content)).sum()
    }

    pub fn total_tokens(&self, messages: &[Message]) -> usize {
        if messages.is_empty() {
            return 0;
        }
        let num_messages = messages.len();
        let message_tokens = self.messages_tokens(messages);
        if messages[num_messages - 1].role.is_user() {
            num_messages * self.per_message_tokens + message_tokens
        } else {
            (num_messages - 1) * self.per_message_tokens + message_tokens
        }
    }

    pub fn max_tokens_limit(&self, messages: &[Message]) -> Result<()> {
        let total_tokens = self.total_tokens(messages) + self.bias_tokens;
        if let Some(max_tokens) = self.max_tokens {
            if total_tokens >= max_tokens {
                bail!("Exceed max tokens limit")
            }
        }
        Ok(())
    }
}
