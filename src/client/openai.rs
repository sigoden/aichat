use super::{prompt_input_api_key, Client, ClientConfig, ExtraConfig, ModelInfo, SendData};

use crate::config::SharedConfig;
use crate::repl::ReplyStreamHandler;

use anyhow::{anyhow, bail, Result};
use async_trait::async_trait;
use eventsource_stream::Eventsource;
use futures_util::StreamExt;
use reqwest::{Client as ReqwestClient, RequestBuilder};
use serde::Deserialize;
use serde_json::{json, Value};
use std::env;

const API_BASE: &str = "https://api.openai.com/v1";

#[derive(Debug)]
pub struct OpenAIClient {
    global_config: SharedConfig,
    config: OpenAIConfig,
    model_info: ModelInfo,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct OpenAIConfig {
    pub name: Option<String>,
    pub api_key: Option<String>,
    pub organization_id: Option<String>,
    pub extra: Option<ExtraConfig>,
}

#[async_trait]
impl Client for OpenAIClient {
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

impl OpenAIClient {
    pub const NAME: &str = "openai";

    pub fn init(global_config: SharedConfig) -> Option<Box<dyn Client>> {
        let model_info = global_config.read().model_info.clone();
        let config = {
            if let ClientConfig::OpenAI(c) = &global_config.read().clients[model_info.index] {
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

    pub fn name(local_config: &OpenAIConfig) -> &str {
        local_config.name.as_deref().unwrap_or(Self::NAME)
    }

    pub fn list_models(local_config: &OpenAIConfig, index: usize) -> Vec<ModelInfo> {
        let client = Self::name(local_config);

        [
            ("gpt-3.5-turbo", 4096),
            ("gpt-3.5-turbo-16k", 16384),
            ("gpt-4", 8192),
            ("gpt-4-32k", 32768),
        ]
        .into_iter()
        .map(|(name, max_tokens)| ModelInfo::new(client, name, max_tokens, index))
        .collect()
    }

    pub fn create_config() -> Result<String> {
        let mut client_config = format!("clients:\n  - type: {}\n", Self::NAME);

        let api_key = prompt_input_api_key()?;
        client_config.push_str(&format!("    api_key: {api_key}\n"));

        Ok(client_config)
    }

    fn request_builder(&self, client: &ReqwestClient, data: SendData) -> Result<RequestBuilder> {
        let env_prefix = Self::name(&self.config).to_uppercase();

        let api_key = self.config.api_key.clone();
        let api_key = api_key
            .or_else(|| env::var(format!("{env_prefix}_API_KEY")).ok())
            .ok_or_else(|| anyhow!("Miss api_key"))?;

        let body = openai_build_body(data, self.model_info.name.clone());

        let api_base = env::var(format!("{env_prefix}_API_BASE"))
            .ok()
            .unwrap_or_else(|| API_BASE.to_string());

        let url = format!("{api_base}/chat/completions");

        let mut builder = client.post(url).bearer_auth(api_key).json(&body);

        if let Some(organization_id) = &self.config.organization_id {
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

pub(crate) fn openai_build_body(data: SendData, model: String) -> Value {
    let SendData {
        messages,
        temperature,
        stream,
    } = data;
    let mut body = json!({
        "model": model,
        "messages": messages,
    });

    if let Some(v) = temperature {
        body.as_object_mut()
            .and_then(|m| m.insert("temperature".into(), json!(v)));
    }

    if stream {
        body.as_object_mut()
            .and_then(|m| m.insert("stream".into(), json!(true)));
    }
    body
}
