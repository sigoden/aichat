use super::{
    message::{Message, MessageContent},
    EmbeddingsData,
};

use crate::utils::{estimate_token_length, format_option_value};

use anyhow::{bail, Result};
use serde::Deserialize;

const PER_MESSAGES_TOKENS: usize = 5;
const BASIS_TOKENS: usize = 2;

#[derive(Debug, Clone)]
pub struct Model {
    client_name: String,
    data: ModelData,
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
            data: ModelData::new(name),
        }
    }

    pub fn from_config(client_name: &str, models: &[ModelData]) -> Vec<Self> {
        models
            .iter()
            .map(|v| Model {
                client_name: client_name.to_string(),
                data: v.clone(),
            })
            .collect()
    }

    pub fn find(models: &[&Self], value: &str) -> Option<Self> {
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
                    model = Some((*found).clone());
                } else if let Some(found) = models.iter().find(|v| v.client_name == client_name) {
                    let mut found = (*found).clone();
                    found.data.name = model_name.to_string();
                    model = Some(found)
                }
            }
            None => {
                if let Some(found) = models.iter().find(|v| v.client_name == client_name) {
                    model = Some((*found).clone());
                }
            }
        }
        model
    }

    pub fn id(&self) -> String {
        format!("{}:{}", self.client_name, self.data.name)
    }

    pub fn client_name(&self) -> &str {
        &self.client_name
    }

    pub fn name(&self) -> &str {
        &self.data.name
    }

    pub fn mode(&self) -> &str {
        &self.data.mode
    }

    pub fn data(&self) -> &ModelData {
        &self.data
    }

    pub fn data_mut(&mut self) -> &mut ModelData {
        &mut self.data
    }

    pub fn description(&self) -> String {
        let ModelData {
            max_input_tokens,
            max_output_tokens,
            input_price,
            output_price,
            supports_vision,
            supports_function_calling,
            ..
        } = &self.data;
        let max_input_tokens = format_option_value(max_input_tokens);
        let max_output_tokens = format_option_value(max_output_tokens);
        let input_price = format_option_value(input_price);
        let output_price = format_option_value(output_price);
        let mut capabilities = vec![];
        if *supports_vision {
            capabilities.push('👁');
        };
        if *supports_function_calling {
            capabilities.push('⚒');
        };
        let capabilities: String = capabilities
            .into_iter()
            .map(|v| format!("{v} "))
            .collect::<Vec<String>>()
            .join("");
        format!(
            "{:>8} / {:>8}  |  {:>6} / {:>6}  {:>6}",
            max_input_tokens, max_output_tokens, input_price, output_price, capabilities
        )
    }

    pub fn max_input_tokens(&self) -> Option<usize> {
        self.data.max_input_tokens
    }

    pub fn max_output_tokens(&self) -> Option<isize> {
        self.data.max_output_tokens
    }

    pub fn supports_vision(&self) -> bool {
        self.data.supports_vision
    }

    pub fn supports_function_calling(&self) -> bool {
        self.data.supports_function_calling
    }

    pub fn default_chunk_size(&self) -> usize {
        self.data.default_chunk_size.unwrap_or(1000)
    }

    pub fn max_concurrent_chunks(&self) -> usize {
        self.data.max_concurrent_chunks.unwrap_or(1)
    }

    pub fn max_tokens_param(&self) -> Option<isize> {
        if self.data.require_max_tokens {
            self.data.max_output_tokens
        } else {
            None
        }
    }

    pub fn set_max_tokens(
        &mut self,
        max_output_tokens: Option<isize>,
        require_max_tokens: bool,
    ) -> &mut Self {
        match max_output_tokens {
            None | Some(0) => self.data.max_output_tokens = None,
            _ => self.data.max_output_tokens = max_output_tokens,
        }
        self.data.require_max_tokens = require_max_tokens;
        self
    }

    pub fn messages_tokens(&self, messages: &[Message]) -> usize {
        messages
            .iter()
            .map(|v| match &v.content {
                MessageContent::Text(text) => estimate_token_length(text),
                MessageContent::Array(_) => 0,
                MessageContent::ToolResults(_) => 0,
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

    pub fn guard_max_input_tokens(&self, messages: &[Message]) -> Result<()> {
        let total_tokens = self.total_tokens(messages) + BASIS_TOKENS;
        if let Some(max_input_tokens) = self.data.max_input_tokens {
            if total_tokens >= max_input_tokens {
                bail!("Exceed max_input_tokens limit")
            }
        }
        Ok(())
    }

    pub fn guard_max_concurrent_chunks(&self, data: &EmbeddingsData) -> Result<()> {
        if data.texts.len() > self.max_concurrent_chunks() {
            bail!("Exceed max_concurrent_chunks limit");
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct ModelData {
    pub name: String,
    #[serde(default = "default_model_mode")]
    pub mode: String,
    pub max_input_tokens: Option<usize>,
    pub input_price: Option<f64>,
    pub output_price: Option<f64>,

    // chat-only properties
    pub max_output_tokens: Option<isize>,
    #[serde(default)]
    pub require_max_tokens: bool,
    #[serde(default)]
    pub supports_vision: bool,
    #[serde(default)]
    pub supports_function_calling: bool,

    // embedding-only properties
    pub default_chunk_size: Option<usize>,
    pub max_concurrent_chunks: Option<usize>,
}

impl ModelData {
    pub fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
            ..Default::default()
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct BuiltinModels {
    pub platform: String,
    pub models: Vec<ModelData>,
}

fn default_model_mode() -> String {
    "chat".into()
}
