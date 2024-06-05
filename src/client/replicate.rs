use super::prompt_format::*;
use super::*;

use anyhow::{anyhow, Result};
use async_trait::async_trait;
use reqwest::{Client as ReqwestClient, RequestBuilder};
use serde::Deserialize;
use serde_json::{json, Value};
use std::time::Duration;

const API_BASE: &str = "https://api.replicate.com/v1";

#[derive(Debug, Clone, Deserialize, Default)]
pub struct ReplicateConfig {
    pub name: Option<String>,
    pub api_key: Option<String>,
    #[serde(default)]
    pub models: Vec<ModelData>,
    pub patches: Option<ModelPatches>,
    pub extra: Option<ExtraConfig>,
}

impl ReplicateClient {
    config_get_fn!(api_key, get_api_key);

    pub const PROMPTS: [PromptAction<'static>; 1] =
        [("api_key", "API Key:", true, PromptKind::String)];

    fn chat_completions_builder(
        &self,
        client: &ReqwestClient,
        data: ChatCompletionsData,
        api_key: &str,
    ) -> Result<RequestBuilder> {
        let mut body = build_chat_completions_body(data, &self.model)?;
        self.patch_chat_completions_body(&mut body);

        let url = format!("{API_BASE}/models/{}/predictions", self.model.name());

        debug!("Replicate Request: {url} {body}");

        let builder = client.post(url).bearer_auth(api_key).json(&body);

        Ok(builder)
    }
}

#[async_trait]
impl Client for ReplicateClient {
    client_common_fns!();

    async fn chat_completions_inner(
        &self,
        client: &ReqwestClient,
        data: ChatCompletionsData,
    ) -> Result<ChatCompletionsOutput> {
        let api_key = self.get_api_key()?;
        let builder = self.chat_completions_builder(client, data, &api_key)?;
        chat_completions(client, builder, &api_key).await
    }

    async fn chat_completions_streaming_inner(
        &self,
        client: &ReqwestClient,
        handler: &mut SseHandler,
        data: ChatCompletionsData,
    ) -> Result<()> {
        let api_key = self.get_api_key()?;
        let builder = self.chat_completions_builder(client, data, &api_key)?;
        chat_completions_streaming(client, builder, handler).await
    }
}

async fn chat_completions(
    client: &ReqwestClient,
    builder: RequestBuilder,
    api_key: &str,
) -> Result<ChatCompletionsOutput> {
    let res = builder.send().await?;
    let status = res.status();
    let data: Value = res.json().await?;
    if !status.is_success() {
        catch_error(&data, status.as_u16())?;
    }
    let prediction_url = data["urls"]["get"]
        .as_str()
        .ok_or_else(|| anyhow!("Invalid response data: {data}"))?;
    loop {
        tokio::time::sleep(Duration::from_millis(500)).await;
        let prediction_data: Value = client
            .get(prediction_url)
            .bearer_auth(api_key)
            .send()
            .await?
            .json()
            .await?;
        debug!("non-stream-data: {prediction_data}");
        let err = || anyhow!("Invalid response data: {prediction_data}");
        let status = prediction_data["status"].as_str().ok_or_else(err)?;
        if status == "succeeded" {
            return extract_chat_completions(&prediction_data);
        } else if status == "failed" || status == "canceled" {
            return Err(err());
        }
    }
}

async fn chat_completions_streaming(
    client: &ReqwestClient,
    builder: RequestBuilder,
    handler: &mut SseHandler,
) -> Result<()> {
    let res = builder.send().await?;
    let status = res.status();
    let data: Value = res.json().await?;
    if !status.is_success() {
        catch_error(&data, status.as_u16())?;
    }
    let stream_url = data["urls"]["stream"]
        .as_str()
        .ok_or_else(|| anyhow!("Invalid response data: {data}"))?;

    let sse_builder = client.get(stream_url).header("accept", "text/event-stream");

    let handle = |message: SseMmessage| -> Result<bool> {
        if message.event == "done" {
            return Ok(true);
        }
        handler.text(&message.data)?;
        Ok(false)
    };
    sse_stream(sse_builder, handle).await
}

fn build_chat_completions_body(data: ChatCompletionsData, model: &Model) -> Result<Value> {
    let ChatCompletionsData {
        messages,
        temperature,
        top_p,
        functions: _,
        stream,
    } = data;

    let prompt = generate_prompt(&messages, smart_prompt_format(model.name()))?;

    let mut input = json!({
        "prompt": prompt,
        "prompt_template": "{prompt}"
    });

    if let Some(v) = model.max_tokens_param() {
        input["max_tokens"] = v.into();
        input["max_new_tokens"] = v.into();
    }
    if let Some(v) = temperature {
        input["temperature"] = v.into();
    }
    if let Some(v) = top_p {
        input["top_p"] = v.into();
    }

    let mut body = json!({
        "input": input,
    });

    if stream {
        body["stream"] = true.into();
    }

    Ok(body)
}

fn extract_chat_completions(data: &Value) -> Result<ChatCompletionsOutput> {
    let text = data["output"]
        .as_array()
        .map(|parts| {
            parts
                .iter()
                .filter_map(|v| v.as_str().map(|v| v.to_string()))
                .collect::<Vec<String>>()
                .join("")
        })
        .ok_or_else(|| anyhow!("Invalid response data: {data}"))?;

    let output = ChatCompletionsOutput {
        text: text.to_string(),
        tool_calls: vec![],
        id: data["id"].as_str().map(|v| v.to_string()),
        input_tokens: data["metrics"]["input_token_count"].as_u64(),
        output_tokens: data["metrics"]["output_token_count"].as_u64(),
    };

    Ok(output)
}
