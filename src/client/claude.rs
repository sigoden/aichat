use super::{
    catch_error, extract_system_message, sse_stream, ClaudeClient, CompletionDetails, ExtraConfig,
    ImageUrl, MessageContent, MessageContentPart, Model, ModelConfig, PromptAction, PromptKind,
    SendData, SsMmessage, SseHandler,
};

use anyhow::{anyhow, bail, Result};
use reqwest::{Client as ReqwestClient, RequestBuilder};
use serde::Deserialize;
use serde_json::{json, Value};

const API_BASE: &str = "https://api.anthropic.com/v1/messages";

#[derive(Debug, Clone, Deserialize)]
pub struct ClaudeConfig {
    pub name: Option<String>,
    pub api_key: Option<String>,
    #[serde(default)]
    pub models: Vec<ModelConfig>,
    pub extra: Option<ExtraConfig>,
}

impl ClaudeClient {
    config_get_fn!(api_key, get_api_key);

    pub const PROMPTS: [PromptAction<'static>; 1] =
        [("api_key", "API Key:", true, PromptKind::String)];

    fn request_builder(&self, client: &ReqwestClient, data: SendData) -> Result<RequestBuilder> {
        let api_key = self.get_api_key().ok();

        let body = claude_build_body(data, &self.model)?;

        let url = API_BASE;

        debug!("Claude Request: {url} {body}");

        let mut builder = client.post(url).json(&body);
        builder = builder.header("anthropic-version", "2023-06-01");
        if let Some(api_key) = api_key {
            builder = builder.header("x-api-key", api_key)
        }

        Ok(builder)
    }
}

impl_client_trait!(
    ClaudeClient,
    claude_send_message,
    claude_send_message_streaming
);

pub async fn claude_send_message(builder: RequestBuilder) -> Result<(String, CompletionDetails)> {
    let res = builder.send().await?;
    let status = res.status();
    let data: Value = res.json().await?;
    if !status.is_success() {
        catch_error(&data, status.as_u16())?;
    }
    claude_extract_completion(&data)
}

pub async fn claude_send_message_streaming(
    builder: RequestBuilder,
    handler: &mut SseHandler,
) -> Result<()> {
    let handle = |message: SsMmessage| -> Result<bool> {
        let data: Value = serde_json::from_str(&message.data)?;
        if let Some(typ) = data["type"].as_str() {
            if typ == "content_block_delta" {
                if let Some(text) = data["delta"]["text"].as_str() {
                    handler.text(text)?;
                }
            }
        }
        Ok(false)
    };

    sse_stream(builder, handle).await
}

pub fn claude_build_body(data: SendData, model: &Model) -> Result<Value> {
    let SendData {
        mut messages,
        temperature,
        top_p,
        stream,
    } = data;

    let system_message = extract_system_message(&mut messages);

    let mut network_image_urls = vec![];
    let messages: Vec<Value> = messages
        .into_iter()
        .map(|message| {
            let role = message.role;
            let content = match message.content {
                MessageContent::Text(text) => vec![json!({"type": "text", "text": text})],
                MessageContent::Array(list) => list
                    .into_iter()
                    .map(|item| match item {
                        MessageContentPart::Text { text } => json!({"type": "text", "text": text}),
                        MessageContentPart::ImageUrl {
                            image_url: ImageUrl { url },
                        } => {
                            if let Some((mime_type, data)) = url
                                .strip_prefix("data:")
                                .and_then(|v| v.split_once(";base64,"))
                            {
                                json!({
                                    "type": "image",
                                    "source": {
                                        "type": "base64",
                                        "media_type": mime_type,
                                        "data": data,
                                    }
                                })
                            } else {
                                network_image_urls.push(url.clone());
                                json!({ "url": url })
                            }
                        }
                    })
                    .collect(),
            };
            json!({ "role": role, "content": content })
        })
        .collect();

    if !network_image_urls.is_empty() {
        bail!(
            "The model does not support network images: {:?}",
            network_image_urls
        );
    }

    let mut body = json!({
        "model": &model.name,
        "messages": messages,
    });
    if let Some(v) = system_message {
        body["system"] = v.into();
    }
    if let Some(v) = model.max_tokens_param() {
        body["max_tokens"] = v.into();
    }
    if let Some(v) = temperature {
        body["temperature"] = v.into();
    }
    if let Some(v) = top_p {
        body["top_p"] = v.into();
    }
    if stream {
        body["stream"] = true.into();
    }
    Ok(body)
}

pub fn claude_extract_completion(data: &Value) -> Result<(String, CompletionDetails)> {
    let text = data["content"][0]["text"]
        .as_str()
        .ok_or_else(|| anyhow!("Invalid response data: {data}"))?;

    let details = CompletionDetails {
        id: data["id"].as_str().map(|v| v.to_string()),
        input_tokens: data["usage"]["input_tokens"].as_u64(),
        output_tokens: data["usage"]["output_tokens"].as_u64(),
    };
    Ok((text.to_string(), details))
}
