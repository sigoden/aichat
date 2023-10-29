use super::openai::{openai_send_message, openai_send_message_streaming};
use super::{set_proxy, Client, ClientConfig, ModelInfo};

use crate::config::SharedConfig;
use crate::repl::ReplyStreamHandler;

use anyhow::{anyhow, Context, Result};
use async_trait::async_trait;
use inquire::{Confirm, Text};
use reqwest::{Client as ReqwestClient, RequestBuilder};
use serde::Deserialize;
use serde_json::json;
use std::env;
use std::time::Duration;

#[allow(clippy::module_name_repetitions)]
#[derive(Debug)]
pub struct AzureOpenAIClient {
    global_config: SharedConfig,
    local_config: AzureOpenAIConfig,
    model_info: ModelInfo,
}

#[derive(Debug, Clone, Deserialize)]
pub struct AzureOpenAIConfig {
    pub api_base: String,
    pub api_key: Option<String>,
    pub models: Vec<AzureOpenAIModel>,
    pub proxy: Option<String>,
    /// Set a timeout in seconds for connect to server
    pub connect_timeout: Option<u64>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct AzureOpenAIModel {
    name: String,
    max_tokens: usize,
}

#[async_trait]
impl Client for AzureOpenAIClient {
    fn get_config(&self) -> &SharedConfig {
        &self.global_config
    }

    async fn send_message_inner(&self, content: &str) -> Result<String> {
        let builder = self.request_builder(content, false)?;
        openai_send_message(builder).await
    }

    async fn send_message_streaming_inner(
        &self,
        content: &str,
        handler: &mut ReplyStreamHandler,
    ) -> Result<()> {
        let builder = self.request_builder(content, true)?;
        openai_send_message_streaming(builder, handler).await
    }
}

impl AzureOpenAIClient {
    pub fn init(global_config: SharedConfig) -> Option<Box<dyn Client>> {
        let model_info = global_config.read().model_info.clone();
        if model_info.client != AzureOpenAIClient::name() {
            return None;
        }
        let local_config = {
            if let ClientConfig::AzureOpenAI(c) = &global_config.read().clients[model_info.index] {
                c.clone()
            } else {
                return None;
            }
        };
        Some(Box::new(Self {
            global_config,
            local_config,
            model_info,
        }))
    }

    pub fn name() -> &'static str {
        "azure-openai"
    }

    pub fn list_models(local_config: &AzureOpenAIConfig, index: usize) -> Vec<ModelInfo> {
        local_config
            .models
            .iter()
            .map(|v| ModelInfo::new(Self::name(), &v.name, v.max_tokens, index))
            .collect()
    }

    pub fn create_config() -> Result<String> {
        let mut client_config = format!("clients:\n  - type: {}\n", Self::name());

        let api_base = Text::new("api_base:")
            .prompt()
            .map_err(|_| anyhow!("An error happened when asking for api base, try again later."))?;

        client_config.push_str(&format!("    api_base: {api_base}\n"));

        if env::var("AZURE_OPENAI_KEY").is_err() {
            let api_key = Text::new("API key:").prompt().map_err(|_| {
                anyhow!("An error happened when asking for api key, try again later.")
            })?;

            client_config.push_str(&format!("    api_key: {api_key}\n"));
        }

        let model_name = Text::new("Model Name:").prompt().map_err(|_| {
            anyhow!("An error happened when asking for model name, try again later.")
        })?;

        let max_tokens = Text::new("Max tokens:").prompt().map_err(|_| {
            anyhow!("An error happened when asking for max tokens, try again later.")
        })?;

        let ans = Confirm::new("Use proxy?")
            .with_default(false)
            .prompt()
            .map_err(|_| anyhow!("Not finish questionnaire, try again later."))?;

        if ans {
            let proxy = Text::new("Set proxy:").prompt().map_err(|_| {
                anyhow!("An error happened when asking for proxy, try again later.")
            })?;
            client_config.push_str(&format!("    proxy: {proxy}\n"));
        }

        client_config.push_str(&format!(
            "    models:\n      - name: {model_name}\n        max_tokens: {max_tokens}\n"
        ));

        Ok(client_config)
    }

    fn request_builder(&self, content: &str, stream: bool) -> Result<RequestBuilder> {
        let messages = self.global_config.read().build_messages(content)?;

        let mut body = json!({
            "messages": messages,
        });

        if let Some(v) = self.global_config.read().get_temperature() {
            body.as_object_mut()
                .and_then(|m| m.insert("temperature".into(), json!(v)));
        }

        if stream {
            body.as_object_mut()
                .and_then(|m| m.insert("stream".into(), json!(true)));
        }

        let client = {
            let mut builder = ReqwestClient::builder();
            builder = set_proxy(builder, &self.local_config.proxy)?;
            let timeout = Duration::from_secs(self.local_config.connect_timeout.unwrap_or(10));
            builder
                .connect_timeout(timeout)
                .build()
                .with_context(|| "Failed to build client")?
        };
        let mut api_base = self.local_config.api_base.clone();
        if !api_base.ends_with('/') {
            api_base = format!("{api_base}/");
        }

        let url = format!(
            "{api_base}openai/deployments/{}/chat/completions?api-version=2023-05-15",
            self.model_info.name
        );

        let mut builder = client.post(url);

        if let Some(api_key) = &self.local_config.api_key {
            builder = builder.header("api-key", api_key)
        } else if let Ok(api_key) = env::var("AZURE_OPENAI_KEY") {
            builder = builder.header("api-key", api_key)
        }
        builder = builder.json(&body);

        Ok(builder)
    }
}
