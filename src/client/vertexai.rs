use super::{
    access_token::*, catch_error, json_stream, message::*, patch_system_message, Client,
    CompletionData, CompletionOutput, ExtraConfig, Model, ModelData, ModelPatches, PromptAction,
    PromptKind, SseHandler, ToolCall, VertexAIClient,
};

use anyhow::{anyhow, bail, Context, Result};
use async_trait::async_trait;
use chrono::{Duration, Utc};
use reqwest::{Client as ReqwestClient, RequestBuilder};
use serde::Deserialize;
use serde_json::{json, Value};
use std::path::PathBuf;

#[derive(Debug, Clone, Deserialize, Default)]
pub struct VertexAIConfig {
    pub name: Option<String>,
    pub project_id: Option<String>,
    pub location: Option<String>,
    pub adc_file: Option<String>,
    #[serde(rename = "safetySettings")]
    pub safety_settings: Option<Value>,
    #[serde(default)]
    pub models: Vec<ModelData>,
    pub patches: Option<ModelPatches>,
    pub extra: Option<ExtraConfig>,
}

impl VertexAIClient {
    config_get_fn!(project_id, get_project_id);
    config_get_fn!(location, get_location);

    pub const PROMPTS: [PromptAction<'static>; 2] = [
        ("project_id", "Project ID", true, PromptKind::String),
        ("location", "Location", true, PromptKind::String),
    ];

    fn request_builder(
        &self,
        client: &ReqwestClient,
        data: CompletionData,
    ) -> Result<RequestBuilder> {
        let project_id = self.get_project_id()?;
        let location = self.get_location()?;
        let access_token = get_access_token(self.name())?;

        let base_url = format!("https://{location}-aiplatform.googleapis.com/v1/projects/{project_id}/locations/{location}/publishers");

        let func = match data.stream {
            true => "streamGenerateContent",
            false => "generateContent",
        };
        let url = format!("{base_url}/google/models/{}:{func}", self.model.name());

        let mut body = gemini_build_body(data, &self.model)?;
        self.patch_request_body(&mut body);

        debug!("VertexAI Request: {url} {body}");

        let builder = client.post(url).bearer_auth(access_token).json(&body);

        Ok(builder)
    }
}

#[async_trait]
impl Client for VertexAIClient {
    client_common_fns!();

    async fn send_message_inner(
        &self,
        client: &ReqwestClient,
        data: CompletionData,
    ) -> Result<CompletionOutput> {
        prepare_gcloud_access_token(client, self.name(), &self.config.adc_file).await?;
        let builder = self.request_builder(client, data)?;
        gemini_send_message(builder).await
    }

    async fn send_message_streaming_inner(
        &self,
        client: &ReqwestClient,
        handler: &mut SseHandler,
        data: CompletionData,
    ) -> Result<()> {
        prepare_gcloud_access_token(client, self.name(), &self.config.adc_file).await?;
        let builder = self.request_builder(client, data)?;
        gemini_send_message_streaming(builder, handler).await
    }
}

pub async fn gemini_send_message(builder: RequestBuilder) -> Result<CompletionOutput> {
    let res = builder.send().await?;
    let status = res.status();
    let data: Value = res.json().await?;
    if !status.is_success() {
        catch_error(&data, status.as_u16())?;
    }
    debug!("non-stream-data: {data}");
    gemini_extract_completion_text(&data)
}

pub async fn gemini_send_message_streaming(
    builder: RequestBuilder,
    handler: &mut SseHandler,
) -> Result<()> {
    let res = builder.send().await?;
    let status = res.status();
    if !status.is_success() {
        let data: Value = res.json().await?;
        catch_error(&data, status.as_u16())?;
    } else {
        let handle = |value: &str| -> Result<()> {
            let data: Value = serde_json::from_str(value)?;
            debug!("stream-data: {data}");
            if let Some(text) = data["candidates"][0]["content"]["parts"][0]["text"].as_str() {
                if !text.is_empty() {
                    handler.text(text)?;
                }
            } else if let Some("SAFETY") = data["promptFeedback"]["blockReason"]
                .as_str()
                .or_else(|| data["candidates"][0]["finishReason"].as_str())
            {
                bail!("Content Blocked")
            } else if let Some(parts) = data["candidates"][0]["content"]["parts"].as_array() {
                for part in parts {
                    if let (Some(name), Some(args)) = (
                        part["functionCall"]["name"].as_str(),
                        part["functionCall"]["args"].as_object(),
                    ) {
                        handler.tool_call(ToolCall::new(name.to_string(), json!(args), None))?;
                    }
                }
            }

            Ok(())
        };
        json_stream(res.bytes_stream(), handle).await?;
    }
    Ok(())
}

fn gemini_extract_completion_text(data: &Value) -> Result<CompletionOutput> {
    let text = data["candidates"][0]["content"]["parts"][0]["text"]
        .as_str()
        .unwrap_or_default();

    let mut tool_calls = vec![];
    if let Some(parts) = data["candidates"][0]["content"]["parts"].as_array() {
        tool_calls = parts
            .iter()
            .filter_map(|part| {
                if let (Some(name), Some(args)) = (
                    part["functionCall"]["name"].as_str(),
                    part["functionCall"]["args"].as_object(),
                ) {
                    Some(ToolCall::new(name.to_string(), json!(args), None))
                } else {
                    None
                }
            })
            .collect()
    }
    if text.is_empty() && tool_calls.is_empty() {
        if let Some("SAFETY") = data["promptFeedback"]["blockReason"]
            .as_str()
            .or_else(|| data["candidates"][0]["finishReason"].as_str())
        {
            bail!("Content Blocked")
        } else {
            bail!("Invalid response data: {data}");
        }
    }
    let output = CompletionOutput {
        text: text.to_string(),
        tool_calls,
        id: None,
        input_tokens: data["usageMetadata"]["promptTokenCount"].as_u64(),
        output_tokens: data["usageMetadata"]["candidatesTokenCount"].as_u64(),
    };
    Ok(output)
}

pub(crate) fn gemini_build_body(data: CompletionData, model: &Model) -> Result<Value> {
    let CompletionData {
        mut messages,
        temperature,
        top_p,
        functions,
        stream: _,
    } = data;

    patch_system_message(&mut messages);

    let mut network_image_urls = vec![];
    let contents: Vec<Value> = messages
        .into_iter()
        .flat_map(|message| {
            let Message { role, content } = message;
            let role = match role {
                MessageRole::User => "user",
                _ => "model",
            };
               match content {
                    MessageContent::Text(text) => vec![json!({
                        "role": role,
                        "parts": [{ "text": text }]
                    })],
                    MessageContent::Array(list) => {
                        let parts: Vec<Value> = list
                            .into_iter()
                            .map(|item| match item {
                                MessageContentPart::Text { text } => json!({"text": text}),
                                MessageContentPart::ImageUrl { image_url: ImageUrl { url } } => {
                                    if let Some((mime_type, data)) = url.strip_prefix("data:").and_then(|v| v.split_once(";base64,")) {
                                        json!({ "inline_data": { "mime_type": mime_type, "data": data } })
                                    } else {
                                        network_image_urls.push(url.clone());
                                        json!({ "url": url })
                                    }
                                },
                            })
                            .collect();
                        vec![json!({ "role": role, "parts": parts })]
                    },
                    MessageContent::ToolResults((tool_call_results, _)) => {
                        let function_call_parts: Vec<Value> = tool_call_results.iter().map(|tool_call_result| {
                            json!({
                                "functionCall": {
                                    "name": tool_call_result.call.name,
                                    "args": tool_call_result.call.arguments,
                                }
                            })
                        }).collect();
                        let function_response_parts: Vec<Value> = tool_call_results.into_iter().map(|tool_call_result| {
                            json!({
                                "functionResponse": {
                                    "name": tool_call_result.call.name,
                                    "response": {
                                        "name": tool_call_result.call.name,
                                        "content": tool_call_result.output,
                                    }
                                }
                            })
                        }).collect();
                        vec![
                            json!({ "role": "model", "parts": function_call_parts }),
                            json!({ "role": "function", "parts": function_response_parts }),
                        ]
                    }
                }
        })
        .collect();

    if !network_image_urls.is_empty() {
        bail!(
            "The model does not support network images: {:?}",
            network_image_urls
        );
    }

    let mut body = json!({ "contents": contents, "generationConfig": {} });

    if let Some(v) = model.max_tokens_param() {
        body["generationConfig"]["maxOutputTokens"] = v.into();
    }
    if let Some(v) = temperature {
        body["generationConfig"]["temperature"] = v.into();
    }
    if let Some(v) = top_p {
        body["generationConfig"]["topP"] = v.into();
    }

    if let Some(functions) = functions {
        body["tools"] = json!([{ "functionDeclarations": *functions }]);
    }

    Ok(body)
}

pub async fn prepare_gcloud_access_token(
    client: &reqwest::Client,
    client_name: &str,
    adc_file: &Option<String>,
) -> Result<()> {
    if !is_valid_access_token(client_name) {
        let (token, expires_in) = fetch_access_token(client, adc_file)
            .await
            .with_context(|| "Failed to fetch access token")?;
        let expires_at = Utc::now()
            + Duration::try_seconds(expires_in)
                .ok_or_else(|| anyhow!("Failed to parse expires_in of access_token"))?;
        set_access_token(client_name, token, expires_at.timestamp())
    }
    Ok(())
}

async fn fetch_access_token(
    client: &reqwest::Client,
    file: &Option<String>,
) -> Result<(String, i64)> {
    let credentials = load_adc(file).await?;
    let value: Value = client
        .post("https://oauth2.googleapis.com/token")
        .json(&credentials)
        .send()
        .await?
        .json()
        .await?;

    if let (Some(access_token), Some(expires_in)) =
        (value["access_token"].as_str(), value["expires_in"].as_i64())
    {
        Ok((access_token.to_string(), expires_in))
    } else if let Some(err_msg) = value["error_description"].as_str() {
        bail!("{err_msg}")
    } else {
        bail!("Invalid response data: {value}")
    }
}

async fn load_adc(file: &Option<String>) -> Result<Value> {
    let adc_file = file
        .as_ref()
        .map(PathBuf::from)
        .or_else(default_adc_file)
        .ok_or_else(|| anyhow!("No application_default_credentials.json"))?;
    let data = tokio::fs::read_to_string(adc_file).await?;
    let data: Value = serde_json::from_str(&data)?;
    if let (Some(client_id), Some(client_secret), Some(refresh_token)) = (
        data["client_id"].as_str(),
        data["client_secret"].as_str(),
        data["refresh_token"].as_str(),
    ) {
        Ok(json!({
            "client_id": client_id,
            "client_secret": client_secret,
            "refresh_token": refresh_token,
            "grant_type": "refresh_token",
        }))
    } else {
        bail!("Invalid application_default_credentials.json")
    }
}

#[cfg(not(windows))]
fn default_adc_file() -> Option<PathBuf> {
    let mut path = dirs::home_dir()?;
    path.push(".config");
    path.push("gcloud");
    path.push("application_default_credentials.json");
    Some(path)
}

#[cfg(windows)]
fn default_adc_file() -> Option<PathBuf> {
    let mut path = dirs::config_dir()?;
    path.push("gcloud");
    path.push("application_default_credentials.json");
    Some(path)
}
