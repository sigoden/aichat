use super::openai::{openai_build_body, openai_send_message, openai_send_message_streaming};
use super::{
    prompt_input_api_base, prompt_input_api_key_optional, prompt_input_max_token,
    prompt_input_model_name, Client, ClientConfig, ExtraConfig, ModelInfo, SendData,
};

use crate::config::SharedConfig;
use crate::repl::ReplyStreamHandler;

use anyhow::Result;
use async_trait::async_trait;
use reqwest::{Client as ReqwestClient, RequestBuilder};
use serde::Deserialize;
use std::env;

#[derive(Debug)]
pub struct LocalAIClient {
    global_config: SharedConfig,
    config: LocalAIConfig,
    model_info: ModelInfo,
}

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
    max_tokens: usize,
}

#[async_trait]
impl Client for LocalAIClient {
    fn config(&self) -> &SharedConfig {
        &self.global_config
    }

    fn extra_config(&self) -> &Option<ExtraConfig> {
        &self.config.extra
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
    pub const NAME: &str = "localai";

    pub fn init(global_config: SharedConfig) -> Option<Box<dyn Client>> {
        let model_info = global_config.read().model_info.clone();
        let config = {
            if let ClientConfig::LocalAI(c) = &global_config.read().clients[model_info.index] {
                c.clone()
            } else {
                return None;
            }
        };
        Some(Box::new(Self {
            global_config,
            config,
            model_info,
        }))
    }

    pub fn name(local_config: &LocalAIConfig) -> &str {
        local_config.name.as_deref().unwrap_or(Self::NAME)
    }

    pub fn list_models(local_config: &LocalAIConfig, index: usize) -> Vec<ModelInfo> {
        let client = Self::name(local_config);

        local_config
            .models
            .iter()
            .map(|v| ModelInfo::new(client, &v.name, v.max_tokens, index))
            .collect()
    }

    pub fn create_config() -> Result<String> {
        let mut client_config = format!("clients:\n  - type: {}\n", Self::NAME);

        let api_base = prompt_input_api_base()?;
        client_config.push_str(&format!("    api_base: {api_base}\n"));

        let api_key = prompt_input_api_key_optional()?;
        client_config.push_str(&format!("    api_key: {api_key}\n"));

        let model_name = prompt_input_model_name()?;

        let max_tokens = prompt_input_max_token()?;

        client_config.push_str(&format!(
            "    models:\n      - name: {model_name}\n        max_tokens: {max_tokens}\n"
        ));

        Ok(client_config)
    }

    fn request_builder(&self, client: &ReqwestClient, data: SendData) -> Result<RequestBuilder> {
        let api_key = self.config.api_key.clone();
        let api_key = api_key.or_else(|| {
            let env_prefix = Self::name(&self.config).to_uppercase();
            env::var(format!("{env_prefix}_API_KEY")).ok()
        });

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
