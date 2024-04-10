use super::{
    json_stream, message::*, patch_system_message, Client, CohereClient, ExtraConfig, Model,
    PromptType, SendData,
};

use crate::{render::ReplyHandler, utils::PromptKind};

use anyhow::{bail, Result};
use async_trait::async_trait;
use reqwest::{Client as ReqwestClient, RequestBuilder};
use serde::Deserialize;
use serde_json::{json, Value};

const API_URL: &str = "https://api.cohere.ai/v1/chat";

const MODELS: [(&str, usize, &str); 2] = [
    // https://docs.cohere.com/docs/command-r
    ("command-r", 128000, "text"),
    ("command-r-plus", 128000, "text"),
];

#[derive(Debug, Clone, Deserialize, Default)]
pub struct CohereConfig {
    pub name: Option<String>,
    pub api_key: Option<String>,
    pub extra: Option<ExtraConfig>,
}

#[async_trait]
impl Client for CohereClient {
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

impl CohereClient {
    config_get_fn!(api_key, get_api_key);

    pub const PROMPTS: [PromptType<'static>; 1] =
        [("api_key", "API Key:", false, PromptKind::String)];

    pub fn list_models(local_config: &CohereConfig) -> Vec<Model> {
        let client_name = Self::name(local_config);
        MODELS
            .into_iter()
            .map(|(name, max_input_tokens, capabilities)| {
                Model::new(client_name, name)
                    .set_capabilities(capabilities.into())
                    .set_max_input_tokens(Some(max_input_tokens))
            })
            .collect()
    }

    fn request_builder(&self, client: &ReqwestClient, data: SendData) -> Result<RequestBuilder> {
        let api_key = self.get_api_key().ok();

        let body = build_body(data, self.model.name.clone())?;

        let url = API_URL;

        debug!("Cohere Request: {url} {body}");

        let mut builder = client.post(url).json(&body);
        if let Some(api_key) = api_key {
            builder = builder.bearer_auth(api_key);
        }

        Ok(builder)
    }
}

pub(crate) async fn send_message(builder: RequestBuilder) -> Result<String> {
    let res = builder.send().await?;
    let status = res.status();
    let data: Value = res.json().await?;
    if status != 200 {
        check_error(&data)?;
    }
    let output = extract_text(&data)?;
    Ok(output.to_string())
}

pub(crate) async fn send_message_streaming(
    builder: RequestBuilder,
    handler: &mut ReplyHandler,
) -> Result<()> {
    let res = builder.send().await?;
    if res.status() != 200 {
        let data: Value = res.json().await?;
        check_error(&data)?;
    } else {
        let handle = |value: &str| -> Result<()> {
            let value: Value = serde_json::from_str(value)?;
            if let Some("text-generation") = value["event_type"].as_str() {
                handler.text(extract_text(&value)?)?;
            }
            Ok(())
        };
        json_stream(res.bytes_stream(), handle).await?;
    }
    Ok(())
}

fn extract_text(data: &Value) -> Result<&str> {
    match data["text"].as_str() {
        Some(text) => Ok(text),
        None => {
            bail!("Invalid response data: {data}")
        }
    }
}

fn check_error(data: &Value) -> Result<()> {
    if let Some(message) = data["message"].as_str() {
        bail!("{message}");
    } else {
        bail!("Error {}", data);
    }
}

pub(crate) fn build_body(data: SendData, model: String) -> Result<Value> {
    let SendData {
        mut messages,
        temperature,
        stream,
    } = data;

    patch_system_message(&mut messages);

    let mut image_urls = vec![];
    let mut messages: Vec<Value> = messages
        .into_iter()
        .map(|message| {
            let role = match message.role {
                MessageRole::User => "USER",
                _ => "CHATBOT",
            };
            match message.content {
                MessageContent::Text(text) => json!({
                    "role": role,
                    "message": text,
                }),
                MessageContent::Array(list) => {
                    let list: Vec<String> = list
                        .into_iter()
                        .filter_map(|item| match item {
                            MessageContentPart::Text { text } => Some(text),
                            MessageContentPart::ImageUrl {
                                image_url: ImageUrl { url },
                            } => {
                                image_urls.push(url.clone());
                                None
                            }
                        })
                        .collect();
                    json!({ "role": role, "message": list.join("\n\n") })
                }
            }
        })
        .collect();

    if !image_urls.is_empty() {
        bail!("The model does not support images: {:?}", image_urls);
    }
    let message = messages.pop().unwrap();
    let message = message["message"].as_str().unwrap_or_default();

    let mut body = json!({
        "model": model,
        "message": message,
    });

    if !messages.is_empty() {
        body["chat_history"] = messages.into();
    }

    if let Some(temperature) = temperature {
        body["temperature"] = temperature.into();
    }
    if stream {
        body["stream"] = true.into();
    }

    Ok(body)
}
