use super::message::{Message, MessageContent};

use crate::utils::count_tokens;

use anyhow::{bail, Result};
use serde::{Deserialize, Deserializer};

const PER_MESSAGES_TOKENS: usize = 5;
const BASIS_TOKENS: usize = 2;

#[derive(Debug, Clone)]
pub struct Model {
    pub client_name: String,
    pub name: String,
    pub max_input_tokens: Option<usize>,
    pub extra_fields: Option<serde_json::Map<String, serde_json::Value>>,
    pub capabilities: ModelCapabilities,
}

impl Default for Model {
    fn default() -> Self {
        Model::new("", "")
    }
}

impl Model {
    pub fn new(client_name: &str, name: &str) -> Self {
        Self {
            client_name: client_name.into(),
            name: name.into(),
            extra_fields: None,
            max_input_tokens: None,
            capabilities: ModelCapabilities::Text,
        }
    }

    pub fn find(models: &[Self], value: &str) -> Option<Self> {
        let mut model = None;
        let (client_name, model_name) = match value.split_once(':') {
            Some((client_name, model_name)) => {
                if model_name.is_empty() {
                    (client_name, None)
                } else {
                    (client_name, Some(model_name))
                }
            }
            None => (value, None),
        };
        match model_name {
            Some(model_name) => {
                if let Some(found) = models.iter().find(|v| v.id() == value) {
                    model = Some(found.clone());
                } else if let Some(found) = models.iter().find(|v| v.client_name == client_name) {
                    let mut found = found.clone();
                    found.name = model_name.to_string();
                    model = Some(found)
                }
            }
            None => {
                if let Some(found) = models.iter().find(|v| v.client_name == client_name) {
                    model = Some(found.clone());
                }
            }
        }
        model
    }

    pub fn id(&self) -> String {
        format!("{}:{}", self.client_name, self.name)
    }

    pub fn set_capabilities(mut self, capabilities: ModelCapabilities) -> Self {
        self.capabilities = capabilities;
        self
    }

    pub fn set_extra_fields(
        mut self,
        extra_fields: Option<serde_json::Map<String, serde_json::Value>>,
    ) -> Self {
        self.extra_fields = extra_fields;
        self
    }

    pub fn set_max_input_tokens(mut self, max_input_tokens: Option<usize>) -> Self {
        match max_input_tokens {
            None | Some(0) => self.max_input_tokens = None,
            _ => self.max_input_tokens = max_input_tokens,
        }
        self
    }

    pub fn messages_tokens(&self, messages: &[Message]) -> usize {
        messages
            .iter()
            .map(|v| {
                match &v.content {
                    MessageContent::Text(text) => count_tokens(text),
                    MessageContent::Array(_) => 0, // TODO
                }
            })
            .sum()
    }

    pub fn total_tokens(&self, messages: &[Message]) -> usize {
        if messages.is_empty() {
            return 0;
        }
        let num_messages = messages.len();
        let message_tokens = self.messages_tokens(messages);
        if messages[num_messages - 1].role.is_user() {
            num_messages * PER_MESSAGES_TOKENS + message_tokens
        } else {
            (num_messages - 1) * PER_MESSAGES_TOKENS + message_tokens
        }
    }

    pub fn max_input_tokens_limit(&self, messages: &[Message]) -> Result<()> {
        let total_tokens = self.total_tokens(messages) + BASIS_TOKENS;
        if let Some(max_input_tokens) = self.max_input_tokens {
            if total_tokens >= max_input_tokens {
                bail!("Exceed max input tokens limit")
            }
        }
        Ok(())
    }

    pub fn merge_extra_fields(&self, body: &mut serde_json::Value) {
        if let (Some(body), Some(extra_fields)) = (body.as_object_mut(), &self.extra_fields) {
            for (k, v) in extra_fields {
                if !body.contains_key(k) {
                    body.insert(k.clone(), v.clone());
                }
            }
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct ModelConfig {
    pub name: String,
    pub max_input_tokens: Option<usize>,
    pub extra_fields: Option<serde_json::Map<String, serde_json::Value>>,
    #[serde(deserialize_with = "deserialize_capabilities")]
    #[serde(default = "default_capabilities")]
    pub capabilities: ModelCapabilities,
}

bitflags::bitflags! {
    #[derive(Debug, Clone, Copy, PartialEq)]
    pub struct ModelCapabilities: u32 {
        const Text = 0b00000001;
        const Vision = 0b00000010;
    }
}

impl From<&str> for ModelCapabilities {
    fn from(value: &str) -> Self {
        let value = if value.is_empty() { "text" } else { value };
        let mut output = ModelCapabilities::empty();
        if value.contains("text") {
            output |= ModelCapabilities::Text;
        }
        if value.contains("vision") {
            output |= ModelCapabilities::Vision;
        }
        output
    }
}

fn deserialize_capabilities<'de, D>(deserializer: D) -> Result<ModelCapabilities, D::Error>
where
    D: Deserializer<'de>,
{
    let value: String = Deserialize::deserialize(deserializer)?;
    Ok(value.as_str().into())
}

fn default_capabilities() -> ModelCapabilities {
    ModelCapabilities::Text
}
