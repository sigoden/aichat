use super::*;

use anyhow::{anyhow, Result};
use reqwest::{Client as ReqwestClient, RequestBuilder};
use serde::Deserialize;
use serde_json::{json, Value};

const API_BASE: &str = "https://api.cloudflare.com/client/v4";

#[derive(Debug, Clone, Deserialize, Default)]
pub struct CloudflareConfig {
    pub name: Option<String>,
    pub account_id: Option<String>,
    pub api_key: Option<String>,
    #[serde(default)]
    pub models: Vec<ModelData>,
    pub patches: Option<ModelPatches>,
    pub extra: Option<ExtraConfig>,
}

impl CloudflareClient {
    config_get_fn!(account_id, get_account_id);
    config_get_fn!(api_key, get_api_key);

    pub const PROMPTS: [PromptAction<'static>; 2] = [
        ("account_id", "Account ID:", true, PromptKind::String),
        ("api_key", "API Key:", true, PromptKind::String),
    ];

    fn chat_completions_builder(
        &self,
        client: &ReqwestClient,
        data: ChatCompletionsData,
    ) -> Result<RequestBuilder> {
        let account_id = self.get_account_id()?;
        let api_key = self.get_api_key()?;

        let mut body = build_chat_completions_body(data, &self.model)?;
        self.patch_chat_completions_body(&mut body);

        let url = format!(
            "{API_BASE}/accounts/{account_id}/ai/run/{}",
            self.model.name()
        );

        debug!("Cloudflare Request: {url} {body}");

        let builder = client.post(url).bearer_auth(api_key).json(&body);

        Ok(builder)
    }
}

impl_client_trait!(
    CloudflareClient,
    chat_completions,
    chat_completions_streaming
);

async fn chat_completions(builder: RequestBuilder) -> Result<ChatCompletionsOutput> {
    let res = builder.send().await?;
    let status = res.status();
    let data: Value = res.json().await?;
    if !status.is_success() {
        catch_error(&data, status.as_u16())?;
    }

    debug!("non-stream-data: {data}");
    extract_chat_completions(&data)
}

async fn chat_completions_streaming(
    builder: RequestBuilder,
    handler: &mut SseHandler,
) -> Result<()> {
    let handle = |message: SseMmessage| -> Result<bool> {
        if message.data == "[DONE]" {
            return Ok(true);
        }
        let data: Value = serde_json::from_str(&message.data)?;
        debug!("stream-data: {data}");
        if let Some(text) = data["response"].as_str() {
            handler.text(text)?;
        }
        Ok(false)
    };
    sse_stream(builder, handle).await
}

fn build_chat_completions_body(data: ChatCompletionsData, model: &Model) -> Result<Value> {
    let ChatCompletionsData {
        messages,
        temperature,
        top_p,
        functions: _,
        stream,
    } = data;

    let mut body = json!({
        "model": &model.name(),
        "messages": messages,
    });

    if let Some(v) = model.max_tokens_param() {
        body["max_tokens"] = v.into();
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

    Ok(body)
}

fn extract_chat_completions(data: &Value) -> Result<ChatCompletionsOutput> {
    let text = data["result"]["response"]
        .as_str()
        .ok_or_else(|| anyhow!("Invalid response data: {data}"))?;

    Ok(ChatCompletionsOutput::new(text))
}
