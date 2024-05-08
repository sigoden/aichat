use super::message::{Message, MessageContent};

use crate::utils::{count_tokens, format_option_value};

use anyhow::{bail, Result};
use serde::Deserialize;

const PER_MESSAGES_TOKENS: usize = 5;
const BASIS_TOKENS: usize = 2;

#[derive(Debug, Clone)]
pub struct Model {
    pub client_name: String,
    pub name: String,
    pub max_input_tokens: Option<usize>,
    pub max_output_tokens: Option<isize>,
    pub pass_max_tokens: bool,
    pub input_price: Option<f64>,
    pub output_price: Option<f64>,
    pub capabilities: ModelCapabilities,
    pub extra_fields: Option<serde_json::Map<String, serde_json::Value>>,
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
            max_input_tokens: None,
            max_output_tokens: None,
            pass_max_tokens: false,
            input_price: None,
            output_price: None,
            capabilities: ModelCapabilities::Text,
            extra_fields: None,
        }
    }

    pub fn from_config(client_name: &str, models: &[ModelConfig]) -> Vec<Self> {
        models
            .iter()
            .map(|v| {
                let mut model = Model::new(client_name, &v.name);
                model
                    .set_max_input_tokens(v.max_input_tokens)
                    .set_max_tokens(v.max_output_tokens, v.pass_max_tokens)
                    .set_input_price(v.input_price)
                    .set_output_price(v.output_price)
                    .set_supports_vision(v.supports_vision)
                    .set_extra_fields(&v.extra_fields);
                model
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
                    found.name = model_name.to_string();
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
        format!("{}:{}", self.client_name, self.name)
    }

    pub fn description(&self) -> String {
        let max_input_tokens = format_option_value(&self.max_input_tokens);
        let max_output_tokens = format_option_value(&self.max_output_tokens);
        let input_price = format_option_value(&self.input_price);
        let output_price = format_option_value(&self.output_price);
        let vision = if self.capabilities.contains(ModelCapabilities::Vision) {
            "👁"
        } else {
            ""
        };
        format!(
            "{:>8} / {:>8}  |  {:>6} / {:>6}  {}",
            max_input_tokens, max_output_tokens, input_price, output_price, vision
        )
    }

    pub fn supports_vision(&self) -> bool {
        self.capabilities.contains(ModelCapabilities::Vision)
    }

    pub fn max_tokens_param(&self) -> Option<isize> {
        if self.pass_max_tokens {
            self.max_output_tokens
        } else {
            None
        }
    }

    pub fn set_max_input_tokens(&mut self, max_input_tokens: Option<usize>) -> &mut Self {
        match max_input_tokens {
            None | Some(0) => self.max_input_tokens = None,
            _ => self.max_input_tokens = max_input_tokens,
        }
        self
    }

    pub fn set_max_tokens(
        &mut self,
        max_output_tokens: Option<isize>,
        pass_max_tokens: bool,
    ) -> &mut Self {
        match max_output_tokens {
            None | Some(0) => self.max_output_tokens = None,
            _ => self.max_output_tokens = max_output_tokens,
        }
        self.pass_max_tokens = pass_max_tokens;
        self
    }

    pub fn set_input_price(&mut self, input_price: Option<f64>) -> &mut Self {
        match input_price {
            None => self.input_price = None,
            _ => self.input_price = input_price,
        }
        self
    }

    pub fn set_output_price(&mut self, output_price: Option<f64>) -> &mut Self {
        match output_price {
            None => self.output_price = None,
            _ => self.output_price = output_price,
        }
        self
    }

    pub fn set_supports_vision(&mut self, supports_vision: bool) -> &mut Self {
        if supports_vision {
            self.capabilities |= ModelCapabilities::Vision;
        } else {
            self.capabilities &= !ModelCapabilities::Vision;
        }
        self
    }

    pub fn set_extra_fields(
        &mut self,
        extra_fields: &Option<serde_json::Map<String, serde_json::Value>>,
    ) -> &mut Self {
        self.extra_fields.clone_from(extra_fields);
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
            for (key, extra_field) in extra_fields {
                if body.contains_key(key) {
                    if let (Some(sub_body), Some(extra_field)) =
                        (body[key].as_object_mut(), extra_field.as_object())
                    {
                        for (subkey, sub_field) in extra_field {
                            if !sub_body.contains_key(subkey) {
                                sub_body.insert(subkey.clone(), sub_field.clone());
                            }
                        }
                    }
                } else {
                    body.insert(key.clone(), extra_field.clone());
                }
            }
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct ModelConfig {
    pub name: String,
    pub max_input_tokens: Option<usize>,
    pub max_output_tokens: Option<isize>,
    pub input_price: Option<f64>,
    pub output_price: Option<f64>,
    #[serde(default)]
    pub supports_vision: bool,
    #[serde(default)]
    pub pass_max_tokens: bool,
    pub extra_fields: Option<serde_json::Map<String, serde_json::Value>>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct BuiltinModels {
    pub platform: String,
    pub models: Vec<ModelConfig>,
}

bitflags::bitflags! {
    #[derive(Debug, Clone, Copy, PartialEq)]
    pub struct ModelCapabilities: u32 {
        const Text = 0b00000001;
        const Vision = 0b00000010;
    }
}
