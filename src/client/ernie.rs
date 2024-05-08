use super::access_token::*;
use super::{
    maybe_catch_error, patch_system_message, sse_stream, Client, CompletionDetails, ErnieClient,
    ExtraConfig, Model, ModelConfig, PromptAction, PromptKind, SendData, SsMmessage, SseHandler,
};

use anyhow::{anyhow, Context, Result};
use async_trait::async_trait;
use reqwest::{Client as ReqwestClient, RequestBuilder};
use serde::Deserialize;
use serde_json::{json, Value};
use std::env;

const API_BASE: &str = "https://aip.baidubce.com/rpc/2.0/ai_custom/v1";
const ACCESS_TOKEN_URL: &str = "https://aip.baidubce.com/oauth/2.0/token";

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
    pub const PROMPTS: [PromptAction<'static>; 2] = [
        ("api_key", "API Key:", true, PromptKind::String),
        ("secret_key", "Secret Key:", true, PromptKind::String),
    ];

    fn request_builder(&self, client: &ReqwestClient, data: SendData) -> Result<RequestBuilder> {
        let body = build_body(data, &self.model);
        let access_token = get_access_token(self.name())?;

        let url = format!(
            "{API_BASE}/wenxinworkshop/chat/{}?access_token={access_token}",
            &self.model.name,
        );

        debug!("Ernie Request: {url} {body}");

        let builder = client.post(url).json(&body);

        Ok(builder)
    }

    async fn prepare_access_token(&self) -> Result<()> {
        let client_name = self.name();
        if !is_valid_access_token(client_name) {
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
            set_access_token(client_name, token, 86400);
        }
        Ok(())
    }
}

#[async_trait]
impl Client for ErnieClient {
    client_common_fns!();

    async fn send_message_inner(
        &self,
        client: &ReqwestClient,
        data: SendData,
    ) -> Result<(String, CompletionDetails)> {
        self.prepare_access_token().await?;
        let builder = self.request_builder(client, data)?;
        send_message(builder).await
    }

    async fn send_message_streaming_inner(
        &self,
        client: &ReqwestClient,
        handler: &mut SseHandler,
        data: SendData,
    ) -> Result<()> {
        self.prepare_access_token().await?;
        let builder = self.request_builder(client, data)?;
        send_message_streaming(builder, handler).await
    }
}

async fn send_message(builder: RequestBuilder) -> Result<(String, CompletionDetails)> {
    let data: Value = builder.send().await?.json().await?;
    maybe_catch_error(&data)?;
    extract_completion_text(&data)
}

async fn send_message_streaming(builder: RequestBuilder, handler: &mut SseHandler) -> Result<()> {
    let handle = |message: SsMmessage| -> Result<bool> {
        let data: Value = serde_json::from_str(&message.data)?;
        if let Some(text) = data["result"].as_str() {
            handler.text(text)?;
        }
        Ok(false)
    };

    sse_stream(builder, handle).await
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

    if let Some(v) = model.max_tokens_param() {
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

fn extract_completion_text(data: &Value) -> Result<(String, CompletionDetails)> {
    let text = data["result"]
        .as_str()
        .ok_or_else(|| anyhow!("Invalid response data: {data}"))?;
    let details = CompletionDetails {
        id: data["id"].as_str().map(|v| v.to_string()),
        input_tokens: data["usage"]["prompt_tokens"].as_u64(),
        output_tokens: data["usage"]["completion_tokens"].as_u64(),
    };
    Ok((text.to_string(), details))
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
