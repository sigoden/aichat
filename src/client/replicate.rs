use std::time::Duration;

use super::{
    catch_error, generate_prompt, smart_prompt_format, sse_stream, Client, CompletionDetails,
    ExtraConfig, Model, ModelConfig, PromptAction, PromptKind, ReplicateClient, SendData,
    SsMmessage, SseHandler,
};

use anyhow::{anyhow, Result};
use async_trait::async_trait;
use reqwest::{Client as ReqwestClient, RequestBuilder};
use serde::Deserialize;
use serde_json::{json, Value};

const API_BASE: &str = "https://api.replicate.com/v1";

#[derive(Debug, Clone, Deserialize, Default)]
pub struct ReplicateConfig {
    pub name: Option<String>,
    pub api_key: Option<String>,
    #[serde(default)]
    pub models: Vec<ModelConfig>,
    pub extra: Option<ExtraConfig>,
}

impl ReplicateClient {
    config_get_fn!(api_key, get_api_key);

    pub const PROMPTS: [PromptAction<'static>; 1] =
        [("api_key", "API Key:", true, PromptKind::String)];

    fn request_builder(
        &self,
        client: &ReqwestClient,
        data: SendData,
        api_key: &str,
    ) -> Result<RequestBuilder> {
        let body = build_body(data, &self.model)?;

        let url = format!("{API_BASE}/models/{}/predictions", self.model.name);

        debug!("Replicate Request: {url} {body}");

        let builder = client.post(url).bearer_auth(api_key).json(&body);

        Ok(builder)
    }
}

#[async_trait]
impl Client for ReplicateClient {
    client_common_fns!();

    async fn send_message_inner(
        &self,
        client: &ReqwestClient,
        data: SendData,
    ) -> Result<(String, CompletionDetails)> {
        let api_key = self.get_api_key()?;
        let builder = self.request_builder(client, data, &api_key)?;
        send_message(client, builder, &api_key).await
    }

    async fn send_message_streaming_inner(
        &self,
        client: &ReqwestClient,
        handler: &mut SseHandler,
        data: SendData,
    ) -> Result<()> {
        let api_key = self.get_api_key()?;
        let builder = self.request_builder(client, data, &api_key)?;
        send_message_streaming(client, builder, handler).await
    }
}

async fn send_message(
    client: &ReqwestClient,
    builder: RequestBuilder,
    api_key: &str,
) -> Result<(String, CompletionDetails)> {
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
        let err = || anyhow!("Invalid response data: {prediction_data}");
        let status = prediction_data["status"].as_str().ok_or_else(err)?;
        if status == "succeeded" {
            return extract_completion(&prediction_data);
        } else if status == "failed" || status == "canceled" {
            return Err(err());
        }
    }
}

async fn send_message_streaming(
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

    let handle = |message: SsMmessage| -> Result<bool> {
        if message.event == "done" {
            return Ok(true);
        }
        handler.text(&message.data)?;
        Ok(false)
    };
    sse_stream(sse_builder, handle).await
}

fn build_body(data: SendData, model: &Model) -> Result<Value> {
    let SendData {
        messages,
        temperature,
        top_p,
        stream,
    } = data;

    let prompt = generate_prompt(&messages, smart_prompt_format(&model.name))?;

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

fn extract_completion(data: &Value) -> Result<(String, CompletionDetails)> {
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

    let details = CompletionDetails {
        id: data["id"].as_str().map(|v| v.to_string()),
        input_tokens: data["metrics"]["input_token_count"].as_u64(),
        output_tokens: data["metrics"]["output_token_count"].as_u64(),
    };

    Ok((text.to_string(), details))
}
