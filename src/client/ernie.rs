use super::access_token::*;
use super::*;

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
    pub models: Vec<ModelData>,
    pub patches: Option<ModelPatches>,
    pub extra: Option<ExtraConfig>,
}

impl ErnieClient {
    pub const PROMPTS: [PromptAction<'static>; 2] = [
        ("api_key", "API Key:", true, PromptKind::String),
        ("secret_key", "Secret Key:", true, PromptKind::String),
    ];

    fn chat_completions_builder(
        &self,
        client: &ReqwestClient,
        data: ChatCompletionsData,
    ) -> Result<RequestBuilder> {
        let mut body = build_chat_completions_body(data, &self.model);
        self.patch_chat_completions_body(&mut body);

        let access_token = get_access_token(self.name())?;

        let url = format!(
            "{API_BASE}/wenxinworkshop/chat/{}?access_token={access_token}",
            &self.model.name(),
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

    async fn chat_completions_inner(
        &self,
        client: &ReqwestClient,
        data: ChatCompletionsData,
    ) -> Result<ChatCompletionsOutput> {
        self.prepare_access_token().await?;
        let builder = self.chat_completions_builder(client, data)?;
        chat_completions(builder).await
    }

    async fn chat_completions_streaming_inner(
        &self,
        client: &ReqwestClient,
        handler: &mut SseHandler,
        data: ChatCompletionsData,
    ) -> Result<()> {
        self.prepare_access_token().await?;
        let builder = self.chat_completions_builder(client, data)?;
        chat_completions_streaming(builder, handler).await
    }
}

async fn chat_completions(builder: RequestBuilder) -> Result<ChatCompletionsOutput> {
    let data: Value = builder.send().await?.json().await?;
    maybe_catch_error(&data)?;
    debug!("non-stream-data: {data}");
    extract_chat_completions_text(&data)
}

async fn chat_completions_streaming(
    builder: RequestBuilder,
    handler: &mut SseHandler,
) -> Result<()> {
    let handle = |message: SseMmessage| -> Result<bool> {
        let data: Value = serde_json::from_str(&message.data)?;
        debug!("stream-data: {data}");
        if let Some(text) = data["result"].as_str() {
            handler.text(text)?;
        }
        Ok(false)
    };

    sse_stream(builder, handle).await
}

fn build_chat_completions_body(data: ChatCompletionsData, model: &Model) -> Value {
    let ChatCompletionsData {
        mut messages,
        temperature,
        top_p,
        functions: _,
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

fn extract_chat_completions_text(data: &Value) -> Result<ChatCompletionsOutput> {
    let text = data["result"]
        .as_str()
        .ok_or_else(|| anyhow!("Invalid response data: {data}"))?;
    let output = ChatCompletionsOutput {
        text: text.to_string(),
        tool_calls: vec![],
        id: data["id"].as_str().map(|v| v.to_string()),
        input_tokens: data["usage"]["prompt_tokens"].as_u64(),
        output_tokens: data["usage"]["completion_tokens"].as_u64(),
    };
    Ok(output)
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
