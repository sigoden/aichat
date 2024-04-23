use super::{ExtraConfig, Model, ModelConfig, OpenAIClient, PromptType, ReplyHandler, SendData};

use crate::utils::PromptKind;

use anyhow::{anyhow, bail, Result};
use async_trait::async_trait;
use futures_util::StreamExt;
use reqwest::{Client as ReqwestClient, RequestBuilder};
use reqwest_eventsource::{Error as EventSourceError, Event, RequestBuilderExt};
use serde::Deserialize;
use serde_json::{json, Value};

const API_BASE: &str = "https://api.openai.com/v1";

const MODELS: [(&str, usize, &str); 8] = [
    // https://platform.openai.com/docs/models
    ("gpt-3.5-turbo", 16385, "text"),
    ("gpt-3.5-turbo-1106", 16385, "text"),
    ("gpt-4-turbo", 128000, "text,vision"),
    ("gpt-4-turbo-preview", 128000, "text"),
    ("gpt-4-1106-preview", 128000, "text"),
    ("gpt-4-vision-preview", 128000, "text,vision"),
    ("gpt-4", 8192, "text"),
    ("gpt-4-32k", 32768, "text"),
];

#[derive(Debug, Clone, Deserialize, Default)]
pub struct OpenAIConfig {
    pub name: Option<String>,
    pub api_key: Option<String>,
    pub api_base: Option<String>,
    pub organization_id: Option<String>,
    #[serde(default)]
    pub models: Vec<ModelConfig>,
    pub extra: Option<ExtraConfig>,
}

openai_compatible_client!(OpenAIClient);

impl OpenAIClient {
    list_models_fn!(OpenAIConfig, &MODELS);
    config_get_fn!(api_key, get_api_key);
    config_get_fn!(api_base, get_api_base);

    pub const PROMPTS: [PromptType<'static>; 1] =
        [("api_key", "API Key:", true, PromptKind::String)];

    fn request_builder(&self, client: &ReqwestClient, data: SendData) -> Result<RequestBuilder> {
        let api_key = self.get_api_key()?;
        let api_base = self.get_api_base().unwrap_or_else(|_| API_BASE.to_string());

        let body = openai_build_body(data, &self.model);

        let url = format!("{api_base}/chat/completions");

        debug!("OpenAI Request: {url} {body}");

        let mut builder = client.post(url).bearer_auth(api_key).json(&body);

        if let Some(organization_id) = &self.config.organization_id {
            builder = builder.header("OpenAI-Organization", organization_id);
        }

        Ok(builder)
    }
}

pub async fn openai_send_message(builder: RequestBuilder) -> Result<String> {
    let data: Value = builder.send().await?.json().await?;
    if let Some(err_msg) = data["error"]["message"].as_str() {
        bail!("{err_msg}");
    }

    let output = data["choices"][0]["message"]["content"]
        .as_str()
        .ok_or_else(|| anyhow!("Invalid response data: {data}"))?;

    Ok(output.to_string())
}

pub async fn openai_send_message_streaming(
    builder: RequestBuilder,
    handler: &mut ReplyHandler,
) -> Result<()> {
    let mut es = builder.eventsource()?;
    while let Some(event) = es.next().await {
        match event {
            Ok(Event::Open) => {}
            Ok(Event::Message(message)) => {
                if message.data == "[DONE]" {
                    break;
                }
                let data: Value = serde_json::from_str(&message.data)?;
                if let Some(text) = data["choices"][0]["delta"]["content"].as_str() {
                    handler.text(text)?;
                }
            }
            Err(err) => {
                match err {
                    EventSourceError::InvalidStatusCode(code, res) => {
                        let text = res.text().await?;
                        let data: Value = match text.parse() {
                            Ok(data) => data,
                            Err(_) => {
                                bail!("Request failed, {code}, {text}");
                            }
                        };
                        if let Some(err_msg) = data["error"]["message"].as_str() {
                            bail!("{err_msg}");
                        } else if let Some(err_msg) = data["message"].as_str() {
                            bail!("{err_msg}");
                        } else {
                            bail!("Request failed, {code}, {text}");
                        }
                    }
                    EventSourceError::StreamEnded => {}
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

pub fn openai_build_body(data: SendData, model: &Model) -> Value {
    let SendData {
        messages,
        temperature,
        stream,
    } = data;

    let mut body = json!({
        "model": &model.name,
        "messages": messages,
    });

    if let Some(max_tokens) = model.max_output_tokens {
        body["max_tokens"] = json!(max_tokens);
    } else if model.name == "gpt-4-vision-preview" {
        // The default max_tokens of gpt-4-vision-preview is only 16, we need to make it larger
        body["max_tokens"] = json!(4096);
    }
    if let Some(v) = temperature {
        body["temperature"] = v.into();
    }
    if stream {
        body["stream"] = true.into();
    }
    body
}
