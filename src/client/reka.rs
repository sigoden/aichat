use super::*;

use anyhow::{anyhow, Result};
use reqwest::{Client as ReqwestClient, RequestBuilder};
use serde::Deserialize;
use serde_json::{json, Value};

const CHAT_COMPLETIONS_API_URL: &str = "https://api.reka.ai/v1/chat";

#[derive(Debug, Clone, Deserialize, Default)]
pub struct RekaConfig {
    pub name: Option<String>,
    pub api_key: Option<String>,
    #[serde(default)]
    pub models: Vec<ModelData>,
    pub patches: Option<ModelPatches>,
    pub extra: Option<ExtraConfig>,
}

impl RekaClient {
    config_get_fn!(api_key, get_api_key);

    pub const PROMPTS: [PromptAction<'static>; 1] =
        [("api_key", "API Key:", true, PromptKind::String)];

    fn chat_completions_builder(
        &self,
        client: &ReqwestClient,
        data: ChatCompletionsData,
    ) -> Result<RequestBuilder> {
        let api_key = self.get_api_key()?;

        let mut body = build_chat_completions_body(data, &self.model);
        self.patch_chat_completions_body(&mut body);

        let url = CHAT_COMPLETIONS_API_URL;

        debug!("Reka Chat Completions Request: {url} {body}");

        let builder = client.post(url).header("x-api-key", api_key).json(&body);

        Ok(builder)
    }
}

impl_client_trait!(RekaClient, chat_completions, chat_completions_streaming);

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
    let mut prev_text = String::new();
    let handle = |message: SseMmessage| -> Result<bool> {
        let data: Value = serde_json::from_str(&message.data)?;
        debug!("stream-data: {data}");
        if let Some(text) = data["responses"][0]["chunk"]["content"].as_str() {
            let delta_text = &text[prev_text.len()..];
            prev_text = text.to_string();
            handler.text(delta_text)?;
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

    body
}

fn extract_chat_completions(data: &Value) -> Result<ChatCompletionsOutput> {
    let text = data["responses"][0]["message"]["content"]
        .as_str()
        .ok_or_else(|| anyhow!("Invalid response data: {data}"))?;

    let output = ChatCompletionsOutput {
        text: text.to_string(),
        tool_calls: vec![],
        id: data["id"].as_str().map(|v| v.to_string()),
        input_tokens: data["usage"]["input_tokens"].as_u64(),
        output_tokens: data["usage"]["output_tokens"].as_u64(),
    };
    Ok(output)
}
