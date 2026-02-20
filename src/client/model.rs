use super::{
    list_all_models, list_client_names,
    message::{Message, MessageContent, MessageContentPart},
    ApiPatch, MessageContentToolCalls, RequestPatch,
};

use crate::config::Config;
use crate::utils::{estimate_token_length, strip_think_tag};

use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fmt::Display;

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

    pub fn retrieve_model(config: &Config, model_id: &str, model_type: ModelType) -> Result<Self> {
        let models = list_all_models(config);
        let (client_name, model_name) = match model_id.split_once(':') {
            Some((client_name, model_name)) => {
                if model_name.is_empty() {
                    (client_name, None)
                } else {
                    (client_name, Some(model_name))
                }
            }
            None => (model_id, None),
        };
        match model_name {
            Some(model_name) => {
                if let Some(model) = models.iter().find(|v| v.id() == model_id) {
                    if model.model_type() == model_type {
                        return Ok((*model).clone());
                    } else {
                        bail!("Model '{model_id}' is not a {model_type} model")
                    }
                }
                if list_client_names(config)
                    .into_iter()
                    .any(|v| *v == client_name)
                    && model_type.can_create_from_name()
                {
                    let mut new_model = Self::new(client_name, model_name);
                    new_model.data.model_type = model_type.to_string();
                    return Ok(new_model);
                }
            }
            None => {
                if let Some(found) = models
                    .iter()
                    .find(|v| v.client_name == client_name && v.model_type() == model_type)
                {
                    return Ok((*found).clone());
                }
            }
        };
        bail!("Unknown {model_type} model '{model_id}'")
    }

    pub fn id(&self) -> String {
        if self.data.name.is_empty() {
            self.client_name.to_string()
        } else {
            format!("{}:{}", self.client_name, self.data.name)
        }
    }

    pub fn client_name(&self) -> &str {
        &self.client_name
    }

    pub fn name(&self) -> &str {
        &self.data.name
    }

    pub fn real_name(&self) -> &str {
        self.data.real_name.as_deref().unwrap_or(&self.data.name)
    }

    pub fn model_type(&self) -> ModelType {
        if self.data.model_type.starts_with("embed") {
            ModelType::Embedding
        } else if self.data.model_type.starts_with("rerank") {
            ModelType::Reranker
        } else {
            ModelType::Chat
        }
    }

    pub fn data(&self) -> &ModelData {
        &self.data
    }

    pub fn data_mut(&mut self) -> &mut ModelData {
        &mut self.data
    }

    pub fn description(&self) -> String {
        match self.model_type() {
            ModelType::Chat => {
                let ModelData {
                    max_input_tokens,
                    max_output_tokens,
                    input_price,
                    output_price,
                    supports_vision,
                    supports_function_calling,
                    ..
                } = &self.data;
                let max_input_tokens = stringify_option_value(max_input_tokens);
                let max_output_tokens = stringify_option_value(max_output_tokens);
                let input_price = stringify_option_value(input_price);
                let output_price = stringify_option_value(output_price);
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
                    "{max_input_tokens:>8} / {max_output_tokens:>8}  |  {input_price:>6} / {output_price:>6}  {capabilities:>6}"
                )
            }
            ModelType::Embedding => {
                let ModelData {
                    input_price,
                    max_tokens_per_chunk,
                    max_batch_size,
                    ..
                } = &self.data;
                let max_tokens = stringify_option_value(max_tokens_per_chunk);
                let max_batch = stringify_option_value(max_batch_size);
                let price = stringify_option_value(input_price);
                format!("max-tokens:{max_tokens};max-batch:{max_batch};price:{price}")
            }
            ModelType::Reranker => String::new(),
        }
    }

    pub fn patch(&self) -> Option<&Value> {
        self.data.patch.as_ref()
    }

    pub fn max_input_tokens(&self) -> Option<usize> {
        self.data.max_input_tokens
    }

    pub fn max_output_tokens(&self) -> Option<isize> {
        self.data.max_output_tokens
    }

    pub fn no_stream(&self) -> bool {
        self.data.no_stream
    }

    pub fn no_system_message(&self) -> bool {
        self.data.no_system_message
    }

    pub fn system_prompt_prefix(&self) -> Option<&str> {
        self.data.system_prompt_prefix.as_deref()
    }

    pub fn max_tokens_per_chunk(&self) -> Option<usize> {
        self.data.max_tokens_per_chunk
    }

    pub fn default_chunk_size(&self) -> usize {
        self.data.default_chunk_size.unwrap_or(1000)
    }

    pub fn max_batch_size(&self) -> Option<usize> {
        self.data.max_batch_size
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
        let messages_len = messages.len();
        messages
            .iter()
            .enumerate()
            .map(|(i, v)| match &v.content {
                MessageContent::Text(text) => {
                    if v.role.is_assistant() && i != messages_len - 1 {
                        estimate_token_length(&strip_think_tag(text))
                    } else {
                        estimate_token_length(text)
                    }
                }
                MessageContent::Array(list) => list
                    .iter()
                    .map(|v| match v {
                        MessageContentPart::Text { text } => estimate_token_length(text),
                        MessageContentPart::ImageUrl { .. } => 0,
                    })
                    .sum(),
                MessageContent::ToolCalls(MessageContentToolCalls {
                    tool_results, text, ..
                }) => {
                    estimate_token_length(text)
                        + tool_results
                            .iter()
                            .map(|v| {
                                serde_json::to_string(v)
                                    .map(|v| estimate_token_length(&v))
                                    .unwrap_or_default()
                            })
                            .sum::<usize>()
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

    pub fn guard_max_input_tokens(&self, messages: &[Message]) -> Result<()> {
        let total_tokens = self.total_tokens(messages) + BASIS_TOKENS;
        if let Some(max_input_tokens) = self.data.max_input_tokens {
            if total_tokens >= max_input_tokens {
                bail!("Exceed max_input_tokens limit")
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ModelData {
    pub name: String,
    #[serde(default = "default_model_type", rename = "type")]
    pub model_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub real_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_input_tokens: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input_price: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_price: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub patch: Option<Value>,

    // chat-only properties
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_output_tokens: Option<isize>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub require_max_tokens: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub supports_vision: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub supports_function_calling: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    no_stream: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    no_system_message: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    system_prompt_prefix: Option<String>,

    // embedding-only properties
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_tokens_per_chunk: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_chunk_size: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_batch_size: Option<usize>,
}

impl ModelData {
    pub fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
            model_type: default_model_type(),
            ..Default::default()
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderModels {
    pub provider: String,
    pub models: Vec<ModelData>,
}

fn default_model_type() -> String {
    "chat".into()
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ModelType {
    Chat,
    Embedding,
    Reranker,
}

impl Display for ModelType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ModelType::Chat => write!(f, "chat"),
            ModelType::Embedding => write!(f, "embedding"),
            ModelType::Reranker => write!(f, "reranker"),
        }
    }
}

impl ModelType {
    pub fn can_create_from_name(self) -> bool {
        match self {
            ModelType::Chat => true,
            ModelType::Embedding => false,
            ModelType::Reranker => true,
        }
    }

    pub fn api_name(self) -> &'static str {
        match self {
            ModelType::Chat => "chat_completions",
            ModelType::Embedding => "embeddings",
            ModelType::Reranker => "rerank",
        }
    }

    pub fn extract_patch(self, patch: &RequestPatch) -> Option<&ApiPatch> {
        match self {
            ModelType::Chat => patch.chat_completions.as_ref(),
            ModelType::Embedding => patch.embeddings.as_ref(),
            ModelType::Reranker => patch.rerank.as_ref(),
        }
    }
}

fn stringify_option_value<T>(value: &Option<T>) -> String
where
    T: std::fmt::Display,
{
    match value {
        Some(value) => value.to_string(),
        None => "-".to_string(),
    }
}
