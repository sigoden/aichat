use super::{
    catch_error, extract_system_message, json_stream, message::*, CohereClient,
    ExtraConfig, Model, ModelConfig, PromptType, ReplyHandler, SendData,
};

use crate::utils::PromptKind;

use anyhow::{bail, Result};
use reqwest::{Client as ReqwestClient, RequestBuilder};
use serde::Deserialize;
use serde_json::{json, Value};

const API_URL: &str = "https://api.cohere.ai/v1/chat";

#[derive(Debug, Clone, Deserialize, Default)]
pub struct CohereConfig {
    pub name: Option<String>,
    pub api_key: Option<String>,
    #[serde(default)]
    pub models: Vec<ModelConfig>,
    pub extra: Option<ExtraConfig>,
}

impl CohereClient {
    list_models_fn!(
        CohereConfig,
        [
            // https://docs.cohere.com/docs/command-r
            ("command-r", "text", 128000),
            ("command-r-plus", "text", 128000),
        ]
    );
    config_get_fn!(api_key, get_api_key);

    pub const PROMPTS: [PromptType<'static>; 1] =
        [("api_key", "API Key:", false, PromptKind::String)];

    fn request_builder(&self, client: &ReqwestClient, data: SendData) -> Result<RequestBuilder> {
        let api_key = self.get_api_key().ok();

        let body = build_body(data, &self.model)?;

        let url = API_URL;

        debug!("Cohere Request: {url} {body}");

        let mut builder = client.post(url).json(&body);
        if let Some(api_key) = api_key {
            builder = builder.bearer_auth(api_key);
        }

        Ok(builder)
    }
}

impl_client_trait!(CohereClient, send_message, send_message_streaming);

async fn send_message(builder: RequestBuilder) -> Result<String> {
    let res = builder.send().await?;
    let status = res.status();
    let data: Value = res.json().await?;
    if status != 200 {
        catch_error(&data, status.as_u16())?;
    }
    let output = extract_text(&data)?;
    Ok(output.to_string())
}

async fn send_message_streaming(builder: RequestBuilder, handler: &mut ReplyHandler) -> Result<()> {
    let res = builder.send().await?;
    let status = res.status();
    if status != 200 {
        let data: Value = res.json().await?;
        catch_error(&data, status.as_u16())?;
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

fn build_body(data: SendData, model: &Model) -> Result<Value> {
    let SendData {
        mut messages,
        temperature,
        top_p,
        stream,
    } = data;

    let system_message = extract_system_message(&mut messages);

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
        "model": &model.name,
        "message": message,
    });

    if let Some(v) = system_message {
        body["preamble"] = v.into();
    }

    if !messages.is_empty() {
        body["chat_history"] = messages.into();
    }

    if let Some(v) = model.max_output_tokens {
        body["max_tokens"] = v.into();
    }
    if let Some(v) = temperature {
        body["temperature"] = v.into();
    }
    if let Some(v) = top_p {
        body["p"] = v.into();
    }
    if stream {
        body["stream"] = true.into();
    }

    Ok(body)
}

fn extract_text(data: &Value) -> Result<&str> {
    match data["text"].as_str() {
        Some(text) => Ok(text),
        None => {
            bail!("Invalid response data: {data}")
        }
    }
}
