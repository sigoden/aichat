use super::openai::{openai_send_message, openai_send_message_streaming};
use super::{set_proxy, Client, ModelInfo};

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
pub struct LocalAIClient {
    global_config: SharedConfig,
    local_config: LocalAIConfig,
    model_info: ModelInfo,
}

#[derive(Debug, Clone, Deserialize)]
pub struct LocalAIConfig {
    pub url: String,
    pub api_key: Option<String>,
    pub models: Vec<LocalAIModel>,
    pub proxy: Option<String>,
    /// Set a timeout in seconds for connect to server
    pub connect_timeout: Option<u64>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct LocalAIModel {
    name: String,
    max_tokens: usize,
}

#[async_trait]
impl Client for LocalAIClient {
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

impl LocalAIClient {
    pub fn new(
        global_config: SharedConfig,
        local_config: LocalAIConfig,
        model_info: ModelInfo,
    ) -> Self {
        Self {
            global_config,
            local_config,
            model_info,
        }
    }

    pub fn name() -> &'static str {
        "localai"
    }

    pub fn list_models(local_config: &LocalAIConfig) -> Vec<(String, usize)> {
        local_config
            .models
            .iter()
            .map(|v| (v.name.to_string(), v.max_tokens))
            .collect()
    }

    pub fn create_config() -> Result<String> {
        let mut client_config = format!("clients:\n  - type: {}\n", Self::name());

        let url = Text::new("URL:")
            .prompt()
            .map_err(|_| anyhow!("An error happened when asking for url, try again later."))?;

        client_config.push_str(&format!("    url: {url}\n"));

        let ans = Confirm::new("Use auth?")
            .with_default(false)
            .prompt()
            .map_err(|_| anyhow!("Not finish questionnaire, try again later."))?;

        if ans {
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
            "model": self.model_info.name,
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

        let mut builder = client.post(&self.local_config.url);
        if let Some(api_key) = &self.local_config.api_key {
            builder = builder.bearer_auth(api_key);
        } else if let Ok(api_key) = env::var("LOCALAI_API_KEY") {
            builder = builder.bearer_auth(api_key);
        }
        builder = builder.json(&body);

        Ok(builder)
    }
}
