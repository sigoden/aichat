use super::{
    catch_error, message::*, CompletionDetails, ExtraConfig, Model, ModelConfig, OllamaClient,
    PromptAction, PromptKind, SendData, SseHandler,
};

use anyhow::{anyhow, bail, Result};
use futures_util::StreamExt;
use reqwest::{Client as ReqwestClient, RequestBuilder};
use serde::Deserialize;
use serde_json::{json, Value};

#[derive(Debug, Clone, Deserialize, Default)]
pub struct OllamaConfig {
    pub name: Option<String>,
    pub api_base: Option<String>,
    pub api_auth: Option<String>,
    pub chat_endpoint: Option<String>,
    pub models: Vec<ModelConfig>,
    pub extra: Option<ExtraConfig>,
}

impl OllamaClient {
    config_get_fn!(api_base, get_api_base);
    config_get_fn!(api_auth, get_api_auth);

    pub const PROMPTS: [PromptAction<'static>; 4] = [
        ("api_base", "API Base:", true, PromptKind::String),
        ("api_auth", "API Auth:", false, PromptKind::String),
        ("models[].name", "Model Name:", true, PromptKind::String),
        (
            "models[].max_input_tokens",
            "Max Input Tokens:",
            false,
            PromptKind::Integer,
        ),
    ];

    fn request_builder(&self, client: &ReqwestClient, data: SendData) -> Result<RequestBuilder> {
        let api_base = self.get_api_base()?;
        let api_auth = self.get_api_auth().ok();

        let mut body = build_body(data, &self.model)?;
        self.model.merge_extra_fields(&mut body);

        let chat_endpoint = self.config.chat_endpoint.as_deref().unwrap_or("/api/chat");

        let url = format!("{api_base}{chat_endpoint}");

        debug!("Ollama Request: {url} {body}");

        let mut builder = client.post(url).json(&body);
        if let Some(api_auth) = api_auth {
            builder = builder.header("Authorization", api_auth)
        }

        Ok(builder)
    }
}

impl_client_trait!(OllamaClient, send_message, send_message_streaming);

async fn send_message(builder: RequestBuilder) -> Result<(String, CompletionDetails)> {
    let res = builder.send().await?;
    let status = res.status();
    let data = res.json().await?;
    if !status.is_success() {
        catch_error(&data, status.as_u16())?;
    }
    let text = data["message"]["content"]
        .as_str()
        .ok_or_else(|| anyhow!("Invalid response data: {data}"))?;
    Ok((text.to_string(), CompletionDetails::default()))
}

async fn send_message_streaming(builder: RequestBuilder, handler: &mut SseHandler) -> Result<()> {
    let res = builder.send().await?;
    let status = res.status();
    if !status.is_success() {
        let data = res.json().await?;
        catch_error(&data, status.as_u16())?;
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

fn build_body(data: SendData, model: &Model) -> Result<Value> {
    let SendData {
        messages,
        temperature,
        top_p,
        stream,
    } = data;

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
        "model": &model.name,
        "messages": messages,
        "stream": stream,
        "options": {},
    });

    if let Some(v) = model.max_tokens_param() {
        body["options"]["num_predict"] = v.into();
    }
    if let Some(v) = temperature {
        body["options"]["temperature"] = v.into();
    }
    if let Some(v) = top_p {
        body["options"]["top_p"] = v.into();
    }

    Ok(body)
}
