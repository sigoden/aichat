use super::{message::*, Client, ExtraConfig, Model, PromptType, QianwenClient, SendData};

use crate::{config::GlobalConfig, render::ReplyHandler, utils::PromptKind};

use anyhow::{anyhow, bail, Result};
use async_trait::async_trait;
use futures_util::StreamExt;
use reqwest::{Client as ReqwestClient, RequestBuilder};
use reqwest_eventsource::{Error as EventSourceError, Event, RequestBuilderExt};
use serde::Deserialize;
use serde_json::{json, Value};

const API_URL: &str =
    "https://dashscope.aliyuncs.com/api/v1/services/aigc/text-generation/generation";

const API_URL_VL: &str =
    "https://dashscope.aliyuncs.com/api/v1/services/aigc/multimodal-generation/generation";

const MODELS: [(&str, usize); 5] = [
    ("qwen-turbo", 8192),
    ("qwen-plus", 32768),
    ("qwen-max", 8192),
    ("qwen-max-longcontext", 30720),
    ("qwen-vl-plus", 0),
];

#[derive(Debug, Clone, Deserialize, Default)]
pub struct QianwenConfig {
    pub name: Option<String>,
    pub api_key: Option<String>,
    pub extra: Option<ExtraConfig>,
}

#[async_trait]
impl Client for QianwenClient {
    fn config(&self) -> (&GlobalConfig, &Option<ExtraConfig>) {
        (&self.global_config, &self.config.extra)
    }

    async fn send_message_inner(&self, client: &ReqwestClient, data: SendData) -> Result<String> {
        let builder = self.request_builder(client, data)?;
        send_message(builder, self.is_vl()).await
    }

    async fn send_message_streaming_inner(
        &self,
        client: &ReqwestClient,
        handler: &mut ReplyHandler,
        data: SendData,
    ) -> Result<()> {
        let builder = self.request_builder(client, data)?;
        send_message_streaming(builder, handler, self.is_vl()).await
    }
}

impl QianwenClient {
    config_get_fn!(api_key, get_api_key);

    pub const PROMPTS: [PromptType<'static>; 1] =
        [("api_key", "API Key:", true, PromptKind::String)];

    pub fn list_models(local_config: &QianwenConfig) -> Vec<Model> {
        let client_name = Self::name(local_config);
        MODELS
            .into_iter()
            .map(|(name, max_tokens)| {
                Model::new(client_name, name).set_max_tokens(Some(max_tokens))
            })
            .collect()
    }

    fn request_builder(&self, client: &ReqwestClient, data: SendData) -> Result<RequestBuilder> {
        let api_key = self.get_api_key()?;

        let stream = data.stream;

        let is_vl = self.is_vl();
        let url = match is_vl {
            true => API_URL_VL,
            false => API_URL,
        };
        let body = build_body(data, self.model.name.clone(), is_vl)?;

        debug!("Qianwen Request: {url} {body}");

        let mut builder = client.post(url).bearer_auth(api_key).json(&body);
        if stream {
            builder = builder.header("X-DashScope-SSE", "enable");
        }

        Ok(builder)
    }

    fn is_vl(&self) -> bool {
        self.model.name.starts_with("qwen-vl")
    }
}

async fn send_message(builder: RequestBuilder, is_vl: bool) -> Result<String> {
    let data: Value = builder.send().await?.json().await?;
    check_error(&data)?;

    let output = if is_vl {
        data["output"]["choices"][0]["message"]["content"][0]["text"].as_str()
    } else {
        data["output"]["text"].as_str()
    };

    let output = output.ok_or_else(|| anyhow!("Unexpected response {data}"))?;

    Ok(output.to_string())
}

async fn send_message_streaming(
    builder: RequestBuilder,
    handler: &mut ReplyHandler,
    is_vl: bool,
) -> Result<()> {
    let mut es = builder.eventsource()?;
    let mut offset = 0;

    while let Some(event) = es.next().await {
        match event {
            Ok(Event::Open) => {}
            Ok(Event::Message(message)) => {
                let data: Value = serde_json::from_str(&message.data)?;
                check_error(&data)?;
                if is_vl {
                    let text = data["output"]["choices"][0]["message"]["content"][0]["text"].as_str();
                    if let Some(text) = text {
                        let text = &text[offset..];
                        handler.text(text)?;
                        offset += text.len();
                    }
                } else if let Some(text) = data["output"]["text"].as_str() {
                    handler.text(text)?;
                }
            }
            Err(err) => {
                match err {
                    EventSourceError::StreamEnded => {}
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

fn check_error(data: &Value) -> Result<()> {
    if let (Some(code), Some(message)) = (data["code"].as_str(), data["message"].as_str()) {
        bail!("{code}: {message}");
    }
    Ok(())
}

fn build_body(data: SendData, model: String, is_vl: bool) -> Result<Value> {
    let SendData {
        messages,
        temperature,
        stream,
    } = data;


    let (input, parameters) = if is_vl {
        let mut exist_embeded_image = false;

        let messages: Vec<Value> = messages
            .into_iter()
            .map(|message| {
                let role = message.role;
                let content = match message.content {
                    MessageContent::Text(text) => vec![json!({"text": text})],
                    MessageContent::Array(list) => list
                        .into_iter()
                        .map(|item| match item {
                            MessageContentPart::Text { text } => json!({"text": text}),
                            MessageContentPart::ImageUrl {
                                image_url: ImageUrl { url },
                            } => {
                                if url.starts_with("data:") {
                                    exist_embeded_image = true;
                                }
                                json!({"image": url})
                            },
                        })
                        .collect(),
                };
                json!({ "role": role, "content": content })
            })
            .collect();

        if exist_embeded_image {
            bail!("The model does not support embeded images");
        }

        let input = json!({
            "messages": messages,
        });

        let mut parameters = json!({});
        if let Some(v) = temperature {
            parameters["top_k"] = ((v * 50.0).round() as usize).into();
        }
        (input, parameters)
    } else {
        let input = json!({
            "messages": messages,
        });

        let mut parameters = json!({});
        if stream {
            parameters["incremental_output"] = true.into();
        }

        if let Some(v) = temperature {
            parameters["temperature"] = v.into();
        }
        (input, parameters)
    };

    let body = json!({
        "model": model,
        "input": input,
        "parameters": parameters
    });
    Ok(body)
}
