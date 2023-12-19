use super::{
    message::*, patch_system_message, Client, ExtraConfig, GeminiClient, Model, PromptType,
    SendData, TokensCountFactors,
};

use crate::{config::GlobalConfig, render::ReplyHandler, utils::PromptKind};

use anyhow::{anyhow, bail, Result};
use async_trait::async_trait;
use futures_util::StreamExt;
use reqwest::{Client as ReqwestClient, RequestBuilder};
use serde::Deserialize;
use serde_json::{json, Value};

const API_BASE: &str = "https://generativelanguage.googleapis.com/v1beta/models/";

const MODELS: [(&str, usize); 3] = [
    ("gemini-pro", 32768),
    ("gemini-pro-vision", 16384),
    ("gemini-ultra", 32768),
];

const TOKENS_COUNT_FACTORS: TokensCountFactors = (5, 2);

#[derive(Debug, Clone, Deserialize, Default)]
pub struct GeminiConfig {
    pub name: Option<String>,
    pub api_key: Option<String>,
    pub extra: Option<ExtraConfig>,
}

#[async_trait]
impl Client for GeminiClient {
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
        send_message_streaming(builder, handler).await
    }
}

impl GeminiClient {
    config_get_fn!(api_key, get_api_key);

    pub const PROMPTS: [PromptType<'static>; 1] =
        [("api_key", "API Key:", true, PromptKind::String)];

    pub fn list_models(local_config: &GeminiConfig) -> Vec<Model> {
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

        let func = match data.stream {
            true => "streamGenerateContent",
            false => "generateContent",
        };

        let body = build_body(data, self.model.name.clone())?;

        let model = self.model.name.clone();

        let url = format!("{API_BASE}{}:{}?key={}", model, func, api_key);

        debug!("Gemini Request: {url} {body}");

        let builder = client.post(url).json(&body);

        Ok(builder)
    }
}

async fn send_message(builder: RequestBuilder) -> Result<String> {
    let res = builder.send().await?;
    let status = res.status();
    let data: Value = res.json().await?;
    if status != 200 {
        check_error(&data)?;
    }
    let output = data["candidates"][0]["content"]["parts"][0]["text"]
        .as_str()
        .ok_or_else(|| anyhow!("Invalid response data: {data}"))?;
    Ok(output.to_string())
}

async fn send_message_streaming(builder: RequestBuilder, handler: &mut ReplyHandler) -> Result<()> {
    let res = builder.send().await?;
    if res.status() != 200 {
        let data: Value = res.json().await?;
        check_error(&data)?;
    } else {
        let mut buffer = vec![];
        let mut cursor = 0;
        let mut start = 0;
        let mut balances = vec![];
        let mut quoting = false;
        let mut stream = res.bytes_stream();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk?;
            let chunk = std::str::from_utf8(&chunk)?;
            buffer.extend(chunk.chars());
            for i in cursor..buffer.len() {
                let ch = buffer[i];
                if quoting {
                    if ch == '"' && buffer[i - 1] != '\\' {
                        quoting = false;
                    }
                    continue;
                }
                match ch {
                    '"' => quoting = true,
                    '{' => {
                        if balances.is_empty() {
                            start = i;
                        }
                        balances.push(ch);
                    }
                    '[' => {
                        if start != 0 {
                            balances.push(ch);
                        }
                    }
                    '}' => {
                        balances.pop();
                        if balances.is_empty() {
                            let value: String = buffer[start..=i].iter().collect();
                            let value: Value = serde_json::from_str(&value)?;
                            if let Some(text) =
                                value["candidates"][0]["content"]["parts"][0]["text"].as_str()
                            {
                                handler.text(text)?;
                            } else {
                                bail!("Invalid response data: {value}")
                            }
                        }
                    }
                    ']' => {
                        balances.pop();
                    }
                    _ => {}
                }
            }
            cursor = buffer.len();
        }
    }
    Ok(())
}

fn check_error(data: &Value) -> Result<()> {
    if let Some((Some(status), Some(message))) = data[0]["error"].as_object().map(|v| {
        (
            v.get("status").and_then(|v| v.as_str()),
            v.get("message").and_then(|v| v.as_str()),
        )
    }) {
        bail!("{status}: {message}")
    } else {
        bail!("Error {}", data);
    }
}

fn build_body(data: SendData, _model: String) -> Result<Value> {
    let SendData {
        mut messages,
        temperature,
        ..
    } = data;

    patch_system_message(&mut messages);

    let mut network_image_urls = vec![];
    let contents: Vec<Value> = messages
        .into_iter()
        .map(|message| {
            let role = match message.role {
                MessageRole::User => "user",
                _ => "model",
            };
            match message.content {
                MessageContent::Text(text) => json!({
                    "role": role,
                    "parts": [{ "text": text }]
                }),
                MessageContent::Array(list) => {
                    let list: Vec<Value> = list
                        .into_iter()
                        .map(|item| match item {
                            MessageContentPart::Text { text } => json!({"text": text}),
                            MessageContentPart::ImageUrl { image_url: ImageUrl { url } } => {
                                if let Some((mime_type, data)) = url.strip_prefix("data:").and_then(|v| v.split_once(";base64,")) {
                                    json!({ "inline_data": { "mime_type": mime_type, "data": data } })
                                } else {
                                    network_image_urls.push(url.clone());
                                    json!({ "url": url })
                                }
                            },
                        })
                        .collect();
                    json!({ "role": role, "parts": list })
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
        "contents": contents,
    });

    if let Some(temperature) = temperature {
        body["generationConfig"] = json!({
            "temperature": temperature,
        });
    }

    Ok(body)
}
