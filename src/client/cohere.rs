use super::{
    catch_error, extract_system_message, json_stream, message::*, CohereClient, CompletionDetails,
    ExtraConfig, Model, ModelConfig, PromptAction, PromptKind, SendData, SseHandler,
};

use anyhow::{anyhow, bail, Result};
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
    config_get_fn!(api_key, get_api_key);

    pub const PROMPTS: [PromptAction<'static>; 1] =
        [("api_key", "API Key:", true, PromptKind::String)];

    fn request_builder(&self, client: &ReqwestClient, data: SendData) -> Result<RequestBuilder> {
        let api_key = self.get_api_key()?;

        let body = build_body(data, &self.model)?;

        let url = API_URL;

        debug!("Cohere Request: {url} {body}");

        let builder = client.post(url).bearer_auth(api_key).json(&body);

        Ok(builder)
    }
}

impl_client_trait!(CohereClient, send_message, send_message_streaming);

async fn send_message(builder: RequestBuilder) -> Result<(String, CompletionDetails)> {
    let res = builder.send().await?;
    let status = res.status();
    let data: Value = res.json().await?;
    if !status.is_success() {
        catch_error(&data, status.as_u16())?;
    }

    extract_completion(&data)
}

async fn send_message_streaming(builder: RequestBuilder, handler: &mut SseHandler) -> Result<()> {
    let res = builder.send().await?;
    let status = res.status();
    if !status.is_success() {
        let data: Value = res.json().await?;
        catch_error(&data, status.as_u16())?;
    } else {
        let handle = |data: &str| -> Result<()> {
            let data: Value = serde_json::from_str(data)?;
            if let Some("text-generation") = data["event_type"].as_str() {
                if let Some(text) = data["text"].as_str() {
                    handler.text(text)?;
                }
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

    if let Some(v) = model.max_tokens_param() {
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

fn extract_completion(data: &Value) -> Result<(String, CompletionDetails)> {
    let text = data["text"]
        .as_str()
        .ok_or_else(|| anyhow!("Invalid response data: {data}"))?;

    let details = CompletionDetails {
        id: data["generation_id"].as_str().map(|v| v.to_string()),
        input_tokens: data["meta"]["billed_units"]["input_tokens"].as_u64(),
        output_tokens: data["meta"]["billed_units"]["output_tokens"].as_u64(),
    };
    Ok((text.to_string(), details))
}
