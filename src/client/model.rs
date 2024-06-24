use super::{
    list_chat_models, list_embedding_models, list_reranker_models,
    message::{Message, MessageContent},
    EmbeddingsData,
};

use crate::config::Config;
use crate::utils::{estimate_token_length, format_option_value};

use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};

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

    pub fn retrieve_chat(config: &Config, model_id: &str) -> Result<Self> {
        match Self::find(&list_chat_models(config), model_id) {
            Some(v) => Ok(v),
            None => bail!("Invalid chat model '{model_id}'"),
        }
    }

    pub fn retrieve_embedding(config: &Config, model_id: &str) -> Result<Self> {
        match Self::find(&list_embedding_models(config), model_id) {
            Some(v) => Ok(v),
            None => bail!("Invalid embedding model '{model_id}'"),
        }
    }

    pub fn retrieve_reranker(config: &Config, model_id: &str) -> Result<Self> {
        match Self::find(&list_reranker_models(config), model_id) {
            Some(v) => Ok(v),
            None => bail!("Invalid reranker model '{model_id}'"),
        }
    }

    pub fn find(models: &[&Self], model_id: &str) -> Option<Self> {
        let mut model = None;
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
                if let Some(found) = models.iter().find(|v| v.id() == model_id) {
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

    pub fn model_type(&self) -> &str {
        &self.data.model_type
    }

    pub fn data(&self) -> &ModelData {
        &self.data
    }

    pub fn data_mut(&mut self) -> &mut ModelData {
        &mut self.data
    }

    pub fn description(&self) -> String {
        match self.model_type() {
            "chat" => {
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
            "embedding" => {
                let ModelData {
                    max_input_tokens,
                    input_price,
                    output_vector_size,
                    max_batch_size,
                    ..
                } = &self.data;
                let dimension = format_option_value(output_vector_size);
                let max_tokens = format_option_value(max_input_tokens);
                let price = format_option_value(input_price);
                let batch = format_option_value(max_batch_size);
                format!(
                    "dimension:{dimension}; max-tokens:{max_tokens}; price:{price}; batch:{batch}"
                )
            }
            _ => String::new(),
        }
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

    pub fn max_batch_size(&self) -> usize {
        self.data.max_batch_size.unwrap_or(1)
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

    pub fn guard_max_batch_size(&self, data: &EmbeddingsData) -> Result<()> {
        if data.texts.len() > self.max_batch_size() {
            bail!("Exceed max_batch_size limit");
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ModelData {
    pub name: String,
    #[serde(default = "default_model_type", rename = "type")]
    pub model_type: String,
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
    pub output_vector_size: Option<usize>,
    pub default_chunk_size: Option<usize>,
    pub max_batch_size: Option<usize>,
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

fn default_model_type() -> String {
    "chat".into()
}
