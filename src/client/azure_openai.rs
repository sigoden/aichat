use super::openai::openai_build_body;
use super::{AzureOpenAIClient, ExtraConfig, Model, ModelConfig, PromptType, SendData};

use crate::utils::PromptKind;

use anyhow::Result;
use async_trait::async_trait;
use reqwest::{Client as ReqwestClient, RequestBuilder};
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct AzureOpenAIConfig {
    pub name: Option<String>,
    pub api_base: Option<String>,
    pub api_key: Option<String>,
    pub models: Vec<ModelConfig>,
    pub extra: Option<ExtraConfig>,
}

openai_compatible_client!(AzureOpenAIClient);

impl AzureOpenAIClient {
    config_get_fn!(api_base, get_api_base);
    config_get_fn!(api_key, get_api_key);

    pub const PROMPTS: [PromptType<'static>; 4] = [
        ("api_base", "API Base:", true, PromptKind::String),
        ("api_key", "API Key:", true, PromptKind::String),
        ("models[].name", "Model Name:", true, PromptKind::String),
        (
            "models[].max_input_tokens",
            "Max Input Tokens:",
            true,
            PromptKind::Integer,
        ),
    ];

    pub fn list_models(local_config: &AzureOpenAIConfig) -> Vec<Model> {
        let client_name = Self::name(local_config);

        local_config
            .models
            .iter()
            .map(|v| {
                Model::new(client_name, &v.name)
                    .set_max_input_tokens(v.max_input_tokens)
                    .set_capabilities(v.capabilities)
            })
            .collect()
    }

    fn request_builder(&self, client: &ReqwestClient, data: SendData) -> Result<RequestBuilder> {
        let api_base = self.get_api_base()?;
        let api_key = self.get_api_key()?;

        let mut body = openai_build_body(data, self.model.name.clone());
        self.model.merge_extra_fields(&mut body);

        let url = format!(
            "{}/openai/deployments/{}/chat/completions?api-version=2023-05-15",
            &api_base, self.model.name
        );

        debug!("AzureOpenAI Request: {url} {body}");

        let builder = client.post(url).header("api-key", api_key).json(&body);

        Ok(builder)
    }
}
