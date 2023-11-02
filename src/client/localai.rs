use super::openai::{openai_build_body, OPENAI_TOKENS_COUNT_FACTORS};
use super::{ExtraConfig, LocalAIClient, PromptType, SendData, Model};

use crate::utils::PromptKind;

use anyhow::Result;
use async_trait::async_trait;
use reqwest::{Client as ReqwestClient, RequestBuilder};
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct LocalAIConfig {
    pub name: Option<String>,
    pub api_base: String,
    pub api_key: Option<String>,
    pub chat_endpoint: Option<String>,
    pub models: Vec<LocalAIModel>,
    pub extra: Option<ExtraConfig>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct LocalAIModel {
    name: String,
    max_tokens: Option<usize>,
}

openai_compatible_client!(LocalAIClient);

impl LocalAIClient {
    config_get_fn!(api_key, get_api_key);

    pub const PROMPTS: [PromptType<'static>; 4] = [
        ("api_base", "API Base:", true, PromptKind::String),
        ("api_key", "API Key:", false, PromptKind::String),
        ("models[].name", "Model Name:", true, PromptKind::String),
        (
            "models[].max_tokens",
            "Max Tokens:",
            false,
            PromptKind::Integer,
        ),
    ];

    pub fn list_models(local_config: &LocalAIConfig, client_index: usize) -> Vec<Model> {
        let client_name = Self::name(local_config);

        local_config
            .models
            .iter()
            .map(|v| {
                Model::new(client_index, client_name, &v.name)
                    .set_max_tokens(v.max_tokens)
                    .set_tokens_count_factors(OPENAI_TOKENS_COUNT_FACTORS)
            })
            .collect()
    }

    fn request_builder(&self, client: &ReqwestClient, data: SendData) -> Result<RequestBuilder> {
        let api_key = self.get_api_key().ok();

        let body = openai_build_body(data, self.model.llm_name.clone());

        let chat_endpoint = self
            .config
            .chat_endpoint
            .as_deref()
            .unwrap_or("/chat/completions");

        let url = format!("{}{chat_endpoint}", self.config.api_base);

        let mut builder = client.post(url).json(&body);
        if let Some(api_key) = api_key {
            builder = builder.bearer_auth(api_key);
        }

        Ok(builder)
    }
}
