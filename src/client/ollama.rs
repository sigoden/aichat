use super::{
    message::*, patch_system_message, Client, ExtraConfig, Model, ModelConfig, OllamaClient,
    PromptType, SendData,
};

use crate::{render::ReplyHandler, utils::PromptKind};

use anyhow::{anyhow, bail, Result};
use async_trait::async_trait;
use futures_util::StreamExt;
use reqwest::{Client as ReqwestClient, RequestBuilder};
use serde::Deserialize;
use serde_json::{json, Value};

#[derive(Debug, Clone, Deserialize, Default)]
pub struct OllamaConfig {
    pub name: Option<String>,
    pub api_base: String,
    pub api_key: Option<String>,
    pub chat_endpoint: Option<String>,
    pub models: Vec<ModelConfig>,
    pub extra: Option<ExtraConfig>,
}

#[async_trait]
impl Client for OllamaClient {
    client_common_fns!();

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
        send_message_streaming(builder, handler).await
    }
}

impl OllamaClient {
    config_get_fn!(api_key, get_api_key);

    pub const PROMPTS: [PromptType<'static>; 4] = [
        ("api_base", "API Base:", true, PromptKind::String),
        ("api_key", "API Key:", false, PromptKind::String),
        ("models[].name", "Model Name:", true, PromptKind::String),
        (
            "models[].max_input_tokens",
            "Max Input Tokens:",
            false,
            PromptKind::Integer,
        ),
    ];

    pub fn list_models(local_config: &OllamaConfig) -> Vec<Model> {
        let client_name = Self::name(local_config);

        local_config
            .models
            .iter()
            .map(|v| {
                Model::new(client_name, &v.name)
                    .set_capabilities(v.capabilities)
                    .set_max_input_tokens(v.max_input_tokens)
                    .set_extra_fields(v.extra_fields.clone())
            })
            .collect()
    }

    fn request_builder(&self, client: &ReqwestClient, data: SendData) -> Result<RequestBuilder> {
        let api_key = self.get_api_key().ok();

        let mut body = build_body(data, self.model.name.clone())?;
        self.model.merge_extra_fields(&mut body);

        let chat_endpoint = self.config.chat_endpoint.as_deref().unwrap_or("/api/chat");

        let url = format!("{}{chat_endpoint}", self.config.api_base);

        debug!("Ollama Request: {url} {body}");

        let mut builder = client.post(url).json(&body);
        if let Some(api_key) = api_key {
            builder = builder.header("Authorization", api_key)
        }

        Ok(builder)
    }
}

async fn send_message(builder: RequestBuilder) -> Result<String> {
    let res = builder.send().await?;
    let status = res.status();
    if status != 200 {
        let text = res.text().await?;
        bail!("{status}, {text}");
    }
    let data: Value = res.json().await?;
    let output = data["message"]["content"]
        .as_str()
        .ok_or_else(|| anyhow!("Invalid response data: {data}"))?;
    Ok(output.to_string())
}

async fn send_message_streaming(builder: RequestBuilder, handler: &mut ReplyHandler) -> Result<()> {
    let res = builder.send().await?;
    let status = res.status();
    if status != 200 {
        let text = res.text().await?;
        bail!("{status}, {text}");
    } else {
        let mut stream = res.bytes_stream();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk?;
            if chunk.is_empty() {
              continue;
            }
            let data: Value = serde_json::from_slice(&chunk)?;
            if data["done"].is_boolean() {
                if let Some(text) = data["message"]["content"].as_str() {
                    handler.text(text)?;
                }
            } else {
                bail!("Invalid response data: {data}")
            }
        }
    }
    Ok(())
}

fn build_body(data: SendData, model: String) -> Result<Value> {
    let SendData {
        mut messages,
        temperature,
        stream,
    } = data;

    patch_system_message(&mut messages);

    let mut network_image_urls = vec![];
    let messages: Vec<Value> = messages
        .into_iter()
        .map(|message| {
            let role = message.role;
            match message.content {
                MessageContent::Text(text) => json!({
                    "role": role,
                    "content": text,
                }),
                MessageContent::Array(list) => {
                    let mut content = vec![];
                    let mut images = vec![];
                    for item in list {
                        match item {
                            MessageContentPart::Text { text } => {
                                content.push(text);
                            }
                            MessageContentPart::ImageUrl {
                                image_url: ImageUrl { url },
                            } => {
                                if let Some((_, data)) = url
                                    .strip_prefix("data:")
                                    .and_then(|v| v.split_once(";base64,"))
                                {
                                    images.push(data.to_string());
                                } else {
                                    network_image_urls.push(url.clone());
                                }
                            }
                        }
                    }
                    let content = content.join("\n\n");
                    json!({ "role": role, "content": content, "images": images })
                }
            }
        })
        .collect();

    if !network_image_urls.is_empty() {
        bail!(
            "The model does not support network images: {:?}",
            network_image_urls
        );
    }

    let mut body = json!({
        "model": model,
        "messages": messages,
        "stream": stream,
    });

    if let Some(temperature) = temperature {
        body["options"] = json!({
            "temperature": temperature,
        });
    }

    Ok(body)
}
