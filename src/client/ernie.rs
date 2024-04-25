use super::{
    maybe_catch_error, patch_system_message, Client, ErnieClient, ExtraConfig, Model, ModelConfig,
    PromptType, ReplyHandler, SendData,
};

use crate::utils::PromptKind;

use anyhow::{anyhow, bail, Context, Result};
use async_trait::async_trait;
use chrono::Utc;
use futures_util::StreamExt;
use reqwest::{Client as ReqwestClient, RequestBuilder};
use reqwest_eventsource::{Error as EventSourceError, Event, RequestBuilderExt};
use serde::Deserialize;
use serde_json::{json, Value};
use std::env;

const API_BASE: &str = "https://aip.baidubce.com/rpc/2.0/ai_custom/v1";
const ACCESS_TOKEN_URL: &str = "https://aip.baidubce.com/oauth/2.0/token";

static mut ACCESS_TOKEN: (String, i64) = (String::new(), 0);

#[derive(Debug, Clone, Deserialize, Default)]
pub struct ErnieConfig {
    pub name: Option<String>,
    pub api_key: Option<String>,
    pub secret_key: Option<String>,
    #[serde(default)]
    pub models: Vec<ModelConfig>,
    pub extra: Option<ExtraConfig>,
}

impl ErnieClient {
    list_models_fn!(
        ErnieConfig,
        [
            // https://cloud.baidu.com/doc/WENXINWORKSHOP/s/clntwmv7t
            ("ernie-4.0-8k", "text", 5120, 2048),
            ("ernie-3.5-8k", "text", 5120, 2048),
            ("ernie-3.5-4k", "text", 2048, 2048),
            ("ernie-speed-8k", "text", 7168, 2048),
            ("ernie-speed-128k", "text", 124000, 4096),
            ("ernie-lite-8k", "text", 7168, 2048),
            ("ernie-tiny-8k", "text", 7168, 2048),
        ]
    );

    pub const PROMPTS: [PromptType<'static>; 2] = [
        ("api_key", "API Key:", true, PromptKind::String),
        ("secret_key", "Secret Key:", true, PromptKind::String),
    ];

    fn request_builder(&self, client: &ReqwestClient, data: SendData) -> Result<RequestBuilder> {
        let body = build_body(data, &self.model);

        let endpoint = match self.model.name.as_str() {
            "ernie-4.0-8k" => "completions_pro",
            "ernie-3.5-8k" => "ernie-3.5-8k-0205",
            "ernie-3.5-4k" => "ernie-3.5-4k-0205",
            "ernie-speed-8k" => "ernie_speed",
            _ => &self.model.name,
        };

        let url = format!(
            "{API_BASE}/wenxinworkshop/chat/{endpoint}?access_token={}",
            unsafe { &ACCESS_TOKEN.0 }
        );

        debug!("Ernie Request: {url} {body}");

        let builder = client.post(url).json(&body);

        Ok(builder)
    }

    async fn prepare_access_token(&self) -> Result<()> {
        if unsafe { ACCESS_TOKEN.0.is_empty() || Utc::now().timestamp() > ACCESS_TOKEN.1 } {
            let env_prefix = Self::name(&self.config).to_uppercase();
            let api_key = self.config.api_key.clone();
            let api_key = api_key
                .or_else(|| env::var(format!("{env_prefix}_API_KEY")).ok())
                .ok_or_else(|| anyhow!("Miss api_key"))?;

            let secret_key = self.config.secret_key.clone();
            let secret_key = secret_key
                .or_else(|| env::var(format!("{env_prefix}_SECRET_KEY")).ok())
                .ok_or_else(|| anyhow!("Miss secret_key"))?;

            let client = self.build_client()?;
            let token = fetch_access_token(&client, &api_key, &secret_key)
                .await
                .with_context(|| "Failed to fetch access token")?;
            unsafe { ACCESS_TOKEN = (token, 86400) };
        }
        Ok(())
    }
}

#[async_trait]
impl Client for ErnieClient {
    client_common_fns!();

    async fn send_message_inner(&self, client: &ReqwestClient, data: SendData) -> Result<String> {
        self.prepare_access_token().await?;
        let builder = self.request_builder(client, data)?;
        send_message(builder).await
    }

    async fn send_message_streaming_inner(
        &self,
        client: &ReqwestClient,
        handler: &mut ReplyHandler,
        data: SendData,
    ) -> Result<()> {
        self.prepare_access_token().await?;
        let builder = self.request_builder(client, data)?;
        send_message_streaming(builder, handler).await
    }
}

async fn send_message(builder: RequestBuilder) -> Result<String> {
    let data: Value = builder.send().await?.json().await?;
    maybe_catch_error(&data)?;

    let output = data["result"]
        .as_str()
        .ok_or_else(|| anyhow!("Unexpected response {data}"))?;

    Ok(output.to_string())
}

async fn send_message_streaming(builder: RequestBuilder, handler: &mut ReplyHandler) -> Result<()> {
    let mut es = builder.eventsource()?;
    while let Some(event) = es.next().await {
        match event {
            Ok(Event::Open) => {}
            Ok(Event::Message(message)) => {
                let data: Value = serde_json::from_str(&message.data)?;
                if let Some(text) = data["result"].as_str() {
                    handler.text(text)?;
                }
            }
            Err(err) => {
                match err {
                    EventSourceError::InvalidContentType(header_value, res) => {
                        let content_type = header_value
                            .to_str()
                            .map_err(|_| anyhow!("Invalid response header"))?;
                        if content_type.contains("application/json") {
                            let data: Value = res.json().await?;
                            maybe_catch_error(&data)?;
                            bail!("Invalid response data: {data}");
                        } else {
                            let text = res.text().await?;
                            if let Some(text) = text.strip_prefix("data: ") {
                                let data: Value = serde_json::from_str(text)?;
                                if let Some(text) = data["result"].as_str() {
                                    handler.text(text)?;
                                }
                            } else {
                                bail!("Invalid response data: {text}")
                            }
                        }
                    }
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

fn build_body(data: SendData, model: &Model) -> Value {
    let SendData {
        mut messages,
        temperature,
        top_p,
        stream,
    } = data;

    patch_system_message(&mut messages);

    let mut body = json!({
        "messages": messages,
    });

    if let Some(v) = model.max_output_tokens {
        body["max_output_tokens"] = v.into();
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

    body
}

async fn fetch_access_token(
    client: &reqwest::Client,
    api_key: &str,
    secret_key: &str,
) -> Result<String> {
    let url = format!("{ACCESS_TOKEN_URL}?grant_type=client_credentials&client_id={api_key}&client_secret={secret_key}");
    let value: Value = client.get(&url).send().await?.json().await?;
    let result = value["access_token"].as_str().ok_or_else(|| {
        if let Some(err_msg) = value["error_description"].as_str() {
            anyhow!("{err_msg}")
        } else {
            anyhow!("Invalid response data")
        }
    })?;
    Ok(result.to_string())
}
