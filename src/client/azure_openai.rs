use super::openai::{openai_build_body, openai_send_message, openai_send_message_streaming};
use super::{AzureOpenAIClient, Client, ExtraConfig, ModelInfo, PromptKind, PromptType, SendData};

use crate::config::SharedConfig;
use crate::repl::ReplyStreamHandler;

use anyhow::{anyhow, Result};
use async_trait::async_trait;
use reqwest::{Client as ReqwestClient, RequestBuilder};
use serde::Deserialize;

use std::env;

#[derive(Debug, Clone, Deserialize)]
pub struct AzureOpenAIConfig {
    pub name: Option<String>,
    pub api_base: Option<String>,
    pub api_key: Option<String>,
    pub models: Vec<AzureOpenAIModel>,
    pub extra: Option<ExtraConfig>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct AzureOpenAIModel {
    name: String,
    max_tokens: Option<usize>,
}

#[async_trait]
impl Client for AzureOpenAIClient {
    fn config(&self) -> (&SharedConfig, &Option<ExtraConfig>) {
        (&self.global_config, &self.config.extra)
    }

    async fn send_message_inner(&self, client: &ReqwestClient, data: SendData) -> Result<String> {
        let builder = self.request_builder(client, data)?;
        openai_send_message(builder).await
    }

    async fn send_message_streaming_inner(
        &self,
        client: &ReqwestClient,
        handler: &mut ReplyStreamHandler,
        data: SendData,
    ) -> Result<()> {
        let builder = self.request_builder(client, data)?;
        openai_send_message_streaming(builder, handler).await
    }
}

impl AzureOpenAIClient {
    config_get_fn!(api_base, get_api_base);

    pub const PROMPTS: [PromptType<'static>; 4] = [
        ("api_base", "API Base:", true, PromptKind::String),
        ("api_key", "API Key:", true, PromptKind::String),
        ("models[].name", "Model Name:", true, PromptKind::String),
        (
            "models[].max_tokens",
            "Max Tokens:",
            true,
            PromptKind::Integer,
        ),
    ];

    pub fn list_models(local_config: &AzureOpenAIConfig, index: usize) -> Vec<ModelInfo> {
        let client = Self::name(local_config);

        local_config
            .models
            .iter()
            .map(|v| ModelInfo::new(client, &v.name, v.max_tokens, index))
            .collect()
    }

    fn request_builder(&self, client: &ReqwestClient, data: SendData) -> Result<RequestBuilder> {
        let api_key = self.config.api_key.clone();
        let api_key = api_key
            .or_else(|| {
                let env_prefix = match &self.config.name {
                    None => "AZURE".into(),
                    Some(v) => v.to_uppercase(),
                };
                env::var(format!("{env_prefix}_OPENAI_KEY")).ok()
            })
            .ok_or_else(|| anyhow!("Miss api_key"))?;

        let api_base = self.get_api_base()?;

        let body = openai_build_body(data, self.model_info.name.clone());

        let url = format!(
            "{}/openai/deployments/{}/chat/completions?api-version=2023-05-15",
            &api_base, self.model_info.name
        );

        let builder = client.post(url).header("api-key", api_key).json(&body);

        Ok(builder)
    }
}
