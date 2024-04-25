use super::{
    catch_error, extract_system_message, ClaudeClient, ExtraConfig, ImageUrl, MessageContent,
    MessageContentPart, Model, ModelConfig, PromptType, ReplyHandler, SendData,
};

use crate::utils::PromptKind;

use anyhow::{anyhow, bail, Result};
use futures_util::StreamExt;
use reqwest::{Client as ReqwestClient, RequestBuilder};
use reqwest_eventsource::{Error as EventSourceError, Event, RequestBuilderExt};
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
    list_models_fn!(
        ClaudeConfig,
        [
            // https://docs.anthropic.com/claude/docs/models-overview
            ("claude-3-opus-20240229", "text,vision", 200000, 4096),
            ("claude-3-sonnet-20240229", "text,vision", 200000, 4096),
            ("claude-3-haiku-20240307", "text,vision", 200000, 4096),
        ]
    );

    config_get_fn!(api_key, get_api_key);

    pub const PROMPTS: [PromptType<'static>; 1] =
        [("api_key", "API Key:", false, PromptKind::String)];

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

pub async fn claude_send_message(builder: RequestBuilder) -> Result<String> {
    let res = builder.send().await?;
    let status = res.status();
    let data: Value = res.json().await?;
    if status != 200 {
        catch_error(&data, status.as_u16())?;
    }

    let output = data["content"][0]["text"]
        .as_str()
        .ok_or_else(|| anyhow!("Invalid response data: {data}"))?;

    Ok(output.to_string())
}

pub async fn claude_send_message_streaming(
    builder: RequestBuilder,
    handler: &mut ReplyHandler,
) -> Result<()> {
    let mut es = builder.eventsource()?;
    while let Some(event) = es.next().await {
        match event {
            Ok(Event::Open) => {}
            Ok(Event::Message(message)) => {
                let data: Value = serde_json::from_str(&message.data)?;
                if let Some(typ) = data["type"].as_str() {
                    if typ == "content_block_delta" {
                        if let Some(text) = data["delta"]["text"].as_str() {
                            handler.text(text)?;
                        }
                    }
                }
            }
            Err(err) => {
                match err {
                    EventSourceError::StreamEnded => {}
                    EventSourceError::InvalidStatusCode(status, res) => {
                        let text = res.text().await?;
                        let data: Value = match text.parse() {
                            Ok(data) => data,
                            Err(_) => {
                                bail!(
                                    "Invalid response data: {text} (status: {})",
                                    status.as_u16()
                                );
                            }
                        };
                        catch_error(&data, status.as_u16())?;
                    }
                    EventSourceError::InvalidContentType(_, res) => {
                        let text = res.text().await?;
                        bail!("The API server should return data as 'text/event-stream', but it isn't. Check the client config. {text}");
                    }
                    _ => {
                        bail!("{}", err);
                    }
                }
                es.close();
            }
        }
    }

    Ok(())
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
    if let Some(v) = model.max_output_tokens {
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
