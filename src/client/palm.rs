use super::{PaLMClient, Client, ExtraConfig, Model, PromptType, SendData, TokensCountFactors, send_message_as_streaming};

use crate::{config::GlobalConfig, render::ReplyHandler, utils::PromptKind};

use anyhow::{anyhow, bail, Result};
use async_trait::async_trait;
use reqwest::{Client as ReqwestClient, RequestBuilder};
use serde::Deserialize;
use serde_json::{json, Value};

const API_BASE: &str = "https://generativelanguage.googleapis.com/v1beta2/models/";

const MODELS: [(&str, usize); 1] = [("chat-bison-001", 4096)];

const TOKENS_COUNT_FACTORS: TokensCountFactors = (3, 8);

#[derive(Debug, Clone, Deserialize, Default)]
pub struct PaLMConfig {
    pub name: Option<String>,
    pub api_key: Option<String>,
    pub extra: Option<ExtraConfig>,
}

#[async_trait]
impl Client for PaLMClient {
    fn config(&self) -> (&GlobalConfig, &Option<ExtraConfig>) {
        (&self.global_config, &self.config.extra)
    }

    async fn send_message_inner(&self, client: &ReqwestClient, data: SendData) -> Result<String> {
        let builder = self.request_builder(client, data)?;
        send_message(builder).await
    }

    async fn send_message_streaming_inner(
        &self,
        client: &ReqwestClient,
        handler: &mut ReplyHandler,
        data: SendData,
    ) -> Result<()> {
        let builder = self.request_builder(client, data)?;
        send_message_as_streaming(builder, handler, send_message).await
    }
}

impl PaLMClient {
    config_get_fn!(api_key, get_api_key);

    pub const PROMPTS: [PromptType<'static>; 1] =
        [("api_key", "API Key:", true, PromptKind::String)];

    pub fn list_models(local_config: &PaLMConfig) -> Vec<Model> {
        let client_name = Self::name(local_config);
        MODELS
            .into_iter()
            .map(|(name, max_tokens)| {
                Model::new(client_name, name)
                    .set_max_tokens(Some(max_tokens))
                    .set_tokens_count_factors(TOKENS_COUNT_FACTORS)
            })
            .collect()
    }

    fn request_builder(&self, client: &ReqwestClient, data: SendData) -> Result<RequestBuilder> {
        let api_key = self.get_api_key()?;

        let body = build_body(data, self.model.name.clone());

        let model = self.model.name.clone();

        let url = format!("{API_BASE}{}:generateMessage?key={}", model, api_key);

        debug!("PaLM Request: {url} {body}");

        let builder = client.post(url).json(&body);

        Ok(builder)
    }
}

async fn send_message(builder: RequestBuilder) -> Result<String> {
    let data: Value = builder.send().await?.json().await?;
    check_error(&data)?;

    let output = data["candidates"][0]["content"]
        .as_str()
        .ok_or_else(|| {
            if let Some(reason) = data["filters"][0]["reason"].as_str() {
                anyhow!("Content Filtering: {reason}")
            } else {
                anyhow!("Unexpected response")
            }
        })?;

    Ok(output.to_string())
}

fn check_error(data: &Value) -> Result<()> {
    if let Some(error) = data["error"].as_object() {
        if let Some(message) = error["message"].as_str() {
            bail!("{message}");
        } else {
            bail!("Error {}", Value::Object(error.clone()));
        }
    }
    Ok(())
}

fn build_body(data: SendData, _model: String) -> Value {
    let SendData {
        mut messages,
        temperature,
        ..
    } = data;

    let mut context = None;
    if messages[0].role.is_system() {
        let message = messages.remove(0);
        context = Some(message.content);
    }
    
    let messages: Vec<Value> = messages.into_iter().map(|v| json!({ "content": v.content })).collect();

    let mut prompt = json!({ "messages": messages });

    if let Some(context) = context {
        prompt["context"] = context.into();
    };

    let mut body = json!({
        "prompt": prompt,
    });

    if let Some(temperature) = temperature {
        body["temperature"] = (temperature / 2.0).into();
    }

    body
}
