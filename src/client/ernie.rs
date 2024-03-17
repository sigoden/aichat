use super::{patch_system_message, Client, ErnieClient, ExtraConfig, Model, PromptType, SendData};

use crate::{render::ReplyHandler, utils::PromptKind};

use anyhow::{anyhow, bail, Context, Result};
use async_trait::async_trait;
use futures_util::StreamExt;
use reqwest::{Client as ReqwestClient, RequestBuilder};
use reqwest_eventsource::{Error as EventSourceError, Event, RequestBuilderExt};
use serde::Deserialize;
use serde_json::{json, Value};
use std::env;

const API_BASE: &str = "https://aip.baidubce.com/rpc/2.0/ai_custom/v1";
const ACCESS_TOKEN_URL: &str = "https://aip.baidubce.com/oauth/2.0/token";

const MODELS: [(&str, usize, &str); 6] = [
    // https://cloud.baidu.com/doc/WENXINWORKSHOP/s/clntwmv7t
    ("ernie-4.0-8k", 5120, "/wenxinworkshop/chat/completions_pro"),
    (
        "ernie-3.5-8k",
        5120,
        "/wenxinworkshop/chat/ernie-3.5-8k-0205",
    ),
    (
        "ernie-3.5-4k",
        2048,
        "/wenxinworkshop/chat/ernie-3.5-4k-0205",
    ),
    ("ernie-speed-8k", 7168, "/wenxinworkshop/chat/ernie_speed"),
    (
        "ernie-speed-128k",
        124000,
        "/wenxinworkshop/chat/ernie-speed-128k",
    ),
    ("ernie-lite-8k", 7168, "/wenxinworkshop/chat/ernie-lite-8k"),
];

static mut ACCESS_TOKEN: String = String::new(); // safe under linear operation

#[derive(Debug, Clone, Deserialize, Default)]
pub struct ErnieConfig {
    pub name: Option<String>,
    pub api_key: Option<String>,
    pub secret_key: Option<String>,
    pub extra: Option<ExtraConfig>,
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

impl ErnieClient {
    pub const PROMPTS: [PromptType<'static>; 2] = [
        ("api_key", "API Key:", true, PromptKind::String),
        ("secret_key", "Secret Key:", true, PromptKind::String),
    ];

    pub fn list_models(local_config: &ErnieConfig) -> Vec<Model> {
        let client_name = Self::name(local_config);
        MODELS
            .into_iter()
            .map(|(name, max_input_tokens, _)| {
                Model::new(client_name, name).set_max_input_tokens(Some(max_input_tokens))
            })
            .collect()
    }

    fn request_builder(&self, client: &ReqwestClient, data: SendData) -> Result<RequestBuilder> {
        let body = build_body(data, self.model.name.clone());

        let model = self.model.name.clone();
        let (_, _, chat_endpoint) = MODELS
            .iter()
            .find(|(v, _, _)| v == &model)
            .ok_or_else(|| anyhow!("Miss Model '{}'", self.model.id()))?;

        let url = format!("{API_BASE}{chat_endpoint}?access_token={}", unsafe {
            &ACCESS_TOKEN
        });

        debug!("Ernie Request: {url} {body}");

        let builder = client.post(url).json(&body);

        Ok(builder)
    }

    async fn prepare_access_token(&self) -> Result<()> {
        if unsafe { ACCESS_TOKEN.is_empty() } {
            // Note: cannot use config_get_fn!
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
            unsafe { ACCESS_TOKEN = token };
        }
        Ok(())
    }
}

async fn send_message(builder: RequestBuilder) -> Result<String> {
    let data: Value = builder.send().await?.json().await?;
    check_error(&data)?;

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
                            check_error(&data)?;
                            bail!("Request failed");
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

fn check_error(data: &Value) -> Result<()> {
    if let Some(err_msg) = data["error_msg"].as_str() {
        if let Some(code) = data["error_code"].as_number().and_then(|v| v.as_u64()) {
            if code == 110 {
                unsafe { ACCESS_TOKEN = String::new() }
            }
            bail!("{err_msg}. err_code: {code}");
        } else {
            bail!("{err_msg}");
        }
    }
    Ok(())
}

fn build_body(data: SendData, _model: String) -> Value {
    let SendData {
        mut messages,
        temperature,
        stream,
    } = data;

    patch_system_message(&mut messages);

    let mut body = json!({
        "messages": messages,
    });

    if let Some(temperature) = temperature {
        body["temperature"] = (temperature / 2.0).into();
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
