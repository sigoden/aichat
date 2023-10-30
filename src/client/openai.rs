use super::{set_proxy, Client, ClientConfig, ModelInfo};

use crate::config::SharedConfig;
use crate::repl::ReplyStreamHandler;

use anyhow::{anyhow, bail, Context, Result};
use async_trait::async_trait;
use eventsource_stream::Eventsource;
use futures_util::StreamExt;
use inquire::{Confirm, Text};
use reqwest::{Client as ReqwestClient, RequestBuilder};
use serde::Deserialize;
use serde_json::{json, Value};
use std::env;
use std::time::Duration;

const API_URL: &str = "https://api.openai.com/v1/chat/completions";

#[derive(Debug)]
pub struct OpenAIClient {
    global_config: SharedConfig,
    local_config: OpenAIConfig,
    model_info: ModelInfo,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct OpenAIConfig {
    pub api_key: Option<String>,
    pub organization_id: Option<String>,
    pub proxy: Option<String>,
    /// Set a timeout in seconds for connect to openai server
    pub connect_timeout: Option<u64>,
}

#[async_trait]
impl Client for OpenAIClient {
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

impl OpenAIClient {
    pub fn init(global_config: SharedConfig) -> Option<Box<dyn Client>> {
        let model_info = global_config.read().model_info.clone();
        if model_info.client != OpenAIClient::name() {
            return None;
        }
        let local_config = {
            if let ClientConfig::OpenAI(c) = &global_config.read().clients[model_info.index] {
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
        "openai"
    }

    pub fn list_models(_local_config: &OpenAIConfig, index: usize) -> Vec<ModelInfo> {
        [
            ("gpt-3.5-turbo", 4096),
            ("gpt-3.5-turbo-16k", 16384),
            ("gpt-4", 8192),
            ("gpt-4-32k", 32768),
        ]
        .into_iter()
        .map(|(name, max_tokens)| ModelInfo::new(Self::name(), name, max_tokens, index))
        .collect()
    }

    pub fn create_config() -> Result<String> {
        let mut client_config = format!("clients:\n  - type: {}\n", Self::name());

        let api_key = Text::new("API key:")
            .prompt()
            .map_err(|_| anyhow!("An error happened when asking for api key, try again later."))?;

        client_config.push_str(&format!("    api_key: {api_key}\n"));

        let ans = Confirm::new("Has Organization?")
            .with_default(false)
            .prompt()
            .map_err(|_| anyhow!("Not finish questionnaire, try again later."))?;

        if ans {
            let organization_id = Text::new("Organization ID:").prompt().map_err(|_| {
                anyhow!("An error happened when asking for proxy, try again later.")
            })?;
            client_config.push_str(&format!("    organization_id: {organization_id}\n"));
        }

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

        Ok(client_config)
    }

    fn request_builder(&self, content: &str, stream: bool) -> Result<RequestBuilder> {
        let api_key = if let Some(api_key) = &self.local_config.api_key {
            api_key.to_string()
        } else if let Ok(api_key) = env::var("OPENAI_API_KEY") {
            api_key.to_string()
        } else {
            bail!("Miss api_key")
        };

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

        let mut builder = client.post(API_URL).bearer_auth(api_key).json(&body);

        if let Some(organization_id) = &self.local_config.organization_id {
            builder = builder.header("OpenAI-Organization", organization_id);
        }

        Ok(builder)
    }
}

pub(crate) async fn openai_send_message(builder: RequestBuilder) -> Result<String> {
    let data: Value = builder.send().await?.json().await?;
    if let Some(err_msg) = data["error"]["message"].as_str() {
        bail!("Request failed, {err_msg}");
    }

    let output = data["choices"][0]["message"]["content"]
        .as_str()
        .ok_or_else(|| anyhow!("Unexpected response {data}"))?;

    Ok(output.to_string())
}

pub(crate) async fn openai_send_message_streaming(
    builder: RequestBuilder,
    handler: &mut ReplyStreamHandler,
) -> Result<()> {
    let res = builder.send().await?;
    if !res.status().is_success() {
        let data: Value = res.json().await?;
        if let Some(err_msg) = data["error"]["message"].as_str() {
            bail!("Request failed, {err_msg}");
        }
        bail!("Request failed");
    }
    let mut stream = res.bytes_stream().eventsource();
    while let Some(part) = stream.next().await {
        let chunk = part?.data;
        if chunk == "[DONE]" {
            break;
        }
        let data: Value = serde_json::from_str(&chunk)?;
        let text = data["choices"][0]["delta"]["content"]
            .as_str()
            .unwrap_or_default();
        if text.is_empty() {
            continue;
        }
        handler.text(text)?;
    }

    Ok(())
}
