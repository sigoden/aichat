use super::openai::{openai_build_body, openai_send_message, openai_send_message_streaming};
use super::{Client, ExtraConfig, LocalAIClient, ModelInfo, PromptKind, PromptType, SendData};

use crate::config::SharedConfig;
use crate::repl::ReplyStreamHandler;

use anyhow::Result;
use async_trait::async_trait;
use reqwest::{Client as ReqwestClient, RequestBuilder};
use serde::Deserialize;
use std::env;

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

#[async_trait]
impl Client for LocalAIClient {
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

    pub fn list_models(local_config: &LocalAIConfig, index: usize) -> Vec<ModelInfo> {
        let client = Self::name(local_config);

        local_config
            .models
            .iter()
            .map(|v| ModelInfo::new(client, &v.name, v.max_tokens, index))
            .collect()
    }

    fn request_builder(&self, client: &ReqwestClient, data: SendData) -> Result<RequestBuilder> {
        let api_key = self.get_api_key().ok();

        let body = openai_build_body(data, self.model_info.name.clone());

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
