use super::openai::{openai_build_body, openai_send_message, openai_send_message_streaming};
use super::{
    prompt_input_api_base, prompt_input_api_key, prompt_input_max_token, prompt_input_model_name,
    Client, ClientConfig, ExtraConfig, ModelInfo, SendData,
};

use crate::config::SharedConfig;
use crate::repl::ReplyStreamHandler;

use anyhow::{anyhow, Result};
use async_trait::async_trait;
use reqwest::{Client as ReqwestClient, RequestBuilder};
use serde::Deserialize;

use std::env;

#[derive(Debug)]
pub struct AzureOpenAIClient {
    global_config: SharedConfig,
    config: AzureOpenAIConfig,
    model_info: ModelInfo,
}

#[derive(Debug, Clone, Deserialize)]
pub struct AzureOpenAIConfig {
    pub name: Option<String>,
    pub api_base: String,
    pub api_key: Option<String>,
    pub models: Vec<AzureOpenAIModel>,
    pub extra: Option<ExtraConfig>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct AzureOpenAIModel {
    name: String,
    max_tokens: usize,
}

#[async_trait]
impl Client for AzureOpenAIClient {
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

impl AzureOpenAIClient {
    pub const NAME: &str = "azure-openai";

    pub fn init(global_config: SharedConfig) -> Option<Box<dyn Client>> {
        let model_info = global_config.read().model_info.clone();
        let config = {
            if let ClientConfig::AzureOpenAI(c) = &global_config.read().clients[model_info.index] {
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

    pub fn name(local_config: &AzureOpenAIConfig) -> &str {
        local_config.name.as_deref().unwrap_or(Self::NAME)
    }

    pub fn list_models(local_config: &AzureOpenAIConfig, index: usize) -> Vec<ModelInfo> {
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

        let api_key = prompt_input_api_key()?;
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
        let api_key = api_key
            .or_else(|| {
                let env_prefix = match &self.config.name {
                    None => "AZURE".into(),
                    Some(v) => v.to_uppercase(),
                };
                env::var(format!("{env_prefix}_OPENAI_KEY")).ok()
            })
            .ok_or_else(|| anyhow!("Miss api_key"))?;

        let body = openai_build_body(data, self.model_info.name.clone());

        let url = format!(
            "{}/openai/deployments/{}/chat/completions?api-version=2023-05-15",
            self.config.api_base, self.model_info.name
        );

        let builder = client.post(url).header("api-key", api_key).json(&body);

        Ok(builder)
    }
}
