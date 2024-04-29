use super::claude::{claude_build_body, claude_send_message, claude_send_message_streaming};
use super::{
    catch_error, json_stream, message::*, patch_system_message, Client, CompletionDetails,
    ExtraConfig, Model, ModelConfig, PromptType, SendData, SseHandler, VertexAIClient,
};

use crate::utils::PromptKind;

use anyhow::{anyhow, bail, Context, Result};
use async_trait::async_trait;
use chrono::{Duration, Utc};
use reqwest::{Client as ReqwestClient, RequestBuilder};
use serde::Deserialize;
use serde_json::{json, Value};
use std::{path::PathBuf, str::FromStr};

static mut ACCESS_TOKEN: (String, i64) = (String::new(), 0); // safe under linear operation

#[derive(Debug, Clone, Deserialize, Default)]
pub struct VertexAIConfig {
    pub name: Option<String>,
    pub project_id: Option<String>,
    pub location: Option<String>,
    pub adc_file: Option<String>,
    pub block_threshold: Option<String>,
    #[serde(default)]
    pub models: Vec<ModelConfig>,
    pub extra: Option<ExtraConfig>,
}

impl VertexAIClient {
    config_get_fn!(project_id, get_project_id);
    config_get_fn!(location, get_location);

    pub const PROMPTS: [PromptType<'static>; 2] = [
        ("project_id", "Project ID", true, PromptKind::String),
        ("location", "Location", true, PromptKind::String),
    ];

    fn request_builder(
        &self,
        client: &ReqwestClient,
        data: SendData,
        model_category: &ModelCategory,
    ) -> Result<RequestBuilder> {
        let project_id = self.get_project_id()?;
        let location = self.get_location()?;

        let base_url = format!("https://{location}-aiplatform.googleapis.com/v1/projects/{project_id}/locations/{location}/publishers");
        let url = build_url(&base_url, &self.model.name, model_category, data.stream)?;

        let block_threshold = self.config.block_threshold.clone();
        let body = build_body(data, &self.model, model_category, block_threshold)?;

        debug!("VertexAI Request: {url} {body}");

        let builder = client
            .post(url)
            .bearer_auth(unsafe { &ACCESS_TOKEN.0 })
            .json(&body);

        Ok(builder)
    }

    async fn prepare_access_token(&self) -> Result<()> {
        if unsafe { ACCESS_TOKEN.0.is_empty() || Utc::now().timestamp() > ACCESS_TOKEN.1 } {
            let client = self.build_client()?;
            let (token, expires_in) = fetch_access_token(&client, &self.config.adc_file)
                .await
                .with_context(|| "Failed to fetch access token")?;
            let expires_at = Utc::now()
                + Duration::try_seconds(expires_in)
                    .ok_or_else(|| anyhow!("Failed to parse expires_in of access_token"))?;
            unsafe { ACCESS_TOKEN = (token, expires_at.timestamp()) };
        }
        Ok(())
    }
}

#[async_trait]
impl Client for VertexAIClient {
    client_common_fns!();

    async fn send_message_inner(
        &self,
        client: &ReqwestClient,
        data: SendData,
    ) -> Result<(String, CompletionDetails)> {
        let model_category = ModelCategory::from_str(&self.model.name)?;
        self.prepare_access_token().await?;
        let builder = self.request_builder(client, data, &model_category)?;
        match model_category {
            ModelCategory::Gemini => gemini_send_message(builder).await,
            ModelCategory::Claude => claude_send_message(builder).await,
        }
    }

    async fn send_message_streaming_inner(
        &self,
        client: &ReqwestClient,
        handler: &mut SseHandler,
        data: SendData,
    ) -> Result<()> {
        let model_category = ModelCategory::from_str(&self.model.name)?;
        self.prepare_access_token().await?;
        let builder = self.request_builder(client, data, &model_category)?;
        match model_category {
            ModelCategory::Gemini => gemini_send_message_streaming(builder, handler).await,
            ModelCategory::Claude => claude_send_message_streaming(builder, handler).await,
        }
    }
}

pub async fn gemini_send_message(builder: RequestBuilder) -> Result<(String, CompletionDetails)> {
    let res = builder.send().await?;
    let status = res.status();
    let data: Value = res.json().await?;
    if !status.is_success() {
        catch_error(&data, status.as_u16())?;
    }
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
            let value: Value = serde_json::from_str(value)?;
            handler.text(gemini_extract_text(&value)?)?;
            Ok(())
        };
        json_stream(res.bytes_stream(), handle).await?;
    }
    Ok(())
}

fn gemini_extract_completion_text(data: &Value) -> Result<(String, CompletionDetails)> {
    let text = gemini_extract_text(data)?;
    let details = CompletionDetails {
        id: None,
        input_tokens: data["usageMetadata"]["promptTokenCount"].as_u64(),
        output_tokens: data["usageMetadata"]["candidatesTokenCount"].as_u64(),
    };
    Ok((text.to_string(), details))
}

fn gemini_extract_text(data: &Value) -> Result<&str> {
    match data["candidates"][0]["content"]["parts"][0]["text"].as_str() {
        Some(text) => Ok(text),
        None => {
            if let Some("SAFETY") = data["promptFeedback"]["blockReason"]
                .as_str()
                .or_else(|| data["candidates"][0]["finishReason"].as_str())
            {
                bail!("Blocked by safety settings，consider adjusting `block_threshold` in the client configuration")
            } else {
                bail!("Invalid response data: {data}")
            }
        }
    }
}

fn build_url(
    base_url: &str,
    model_name: &str,
    model_category: &ModelCategory,
    stream: bool,
) -> Result<String> {
    let url = match model_category {
        ModelCategory::Gemini => {
            let func = match stream {
                true => "streamGenerateContent",
                false => "generateContent",
            };
            format!("{base_url}/google/models/{model_name}:{func}")
        }
        ModelCategory::Claude => {
            format!("{base_url}/anthropic/models/{model_name}:streamRawPredict")
        }
    };
    Ok(url)
}

fn build_body(
    data: SendData,
    model: &Model,
    model_category: &ModelCategory,
    block_threshold: Option<String>,
) -> Result<Value> {
    match model_category {
        ModelCategory::Gemini => gemini_build_body(data, model, block_threshold),
        ModelCategory::Claude => {
            let mut body = claude_build_body(data, model)?;
            if let Some(body_obj) = body.as_object_mut() {
                body_obj.remove("model");
            }
            body["anthropic_version"] = "vertex-2023-10-16".into();
            Ok(body)
        }
    }
}

pub(crate) fn gemini_build_body(
    data: SendData,
    model: &Model,
    block_threshold: Option<String>,
) -> Result<Value> {
    let SendData {
        mut messages,
        temperature,
        top_p,
        stream: _,
    } = data;

    patch_system_message(&mut messages);

    let mut network_image_urls = vec![];
    let contents: Vec<Value> = messages
        .into_iter()
        .map(|message| {
            let role = match message.role {
                MessageRole::User => "user",
                _ => "model",
            };
            match message.content {
                MessageContent::Text(text) => json!({
                    "role": role,
                    "parts": [{ "text": text }]
                }),
                MessageContent::Array(list) => {
                    let list: Vec<Value> = list
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
                    json!({ "role": role, "parts": list })
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

    if let Some(block_threshold) = block_threshold {
        body["safetySettings"] = json!([
            {"category":"HARM_CATEGORY_HARASSMENT","threshold":block_threshold},
            {"category":"HARM_CATEGORY_HATE_SPEECH","threshold":block_threshold},
            {"category":"HARM_CATEGORY_SEXUALLY_EXPLICIT","threshold":block_threshold},
            {"category":"HARM_CATEGORY_DANGEROUS_CONTENT","threshold":block_threshold}
        ]);
    }

    if let Some(v) = model.max_output_tokens {
        body["generationConfig"]["maxOutputTokens"] = v.into();
    }
    if let Some(v) = temperature {
        body["generationConfig"]["temperature"] = v.into();
    }
    if let Some(v) = top_p {
        body["generationConfig"]["topP"] = v.into();
    }

    Ok(body)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ModelCategory {
    Gemini,
    Claude,
}

impl FromStr for ModelCategory {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        if s.starts_with("gemini-") {
            Ok(ModelCategory::Gemini)
        } else if s.starts_with("claude-") {
            Ok(ModelCategory::Claude)
        } else {
            unsupported_model!(s)
        }
    }
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
