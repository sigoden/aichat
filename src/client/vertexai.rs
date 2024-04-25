use super::{
    catch_error, json_stream, message::*, patch_system_message, Client, ExtraConfig, Model,
    ModelConfig, PromptType, ReplyHandler, SendData, VertexAIClient,
};

use crate::utils::PromptKind;

use anyhow::{anyhow, bail, Context, Result};
use async_trait::async_trait;
use chrono::{Duration, Utc};
use reqwest::{Client as ReqwestClient, RequestBuilder};
use serde::Deserialize;
use serde_json::{json, Value};
use std::path::PathBuf;

static mut ACCESS_TOKEN: (String, i64) = (String::new(), 0); // safe under linear operation

#[derive(Debug, Clone, Deserialize, Default)]
pub struct VertexAIConfig {
    pub name: Option<String>,
    pub api_base: Option<String>,
    pub adc_file: Option<String>,
    pub block_threshold: Option<String>,
    #[serde(default)]
    pub models: Vec<ModelConfig>,
    pub extra: Option<ExtraConfig>,
}

impl VertexAIClient {
    list_models_fn!(
        VertexAIConfig,
        [
            // https://cloud.google.com/vertex-ai/generative-ai/docs/learn/models
            ("gemini-1.0-pro", "text", 24568),
            ("gemini-1.0-pro-vision", "text,vision", 14336),
            ("gemini-1.5-pro-preview-0409", "text,vision", 1000000),
        ]
    );
    config_get_fn!(api_base, get_api_base);

    pub const PROMPTS: [PromptType<'static>; 1] =
        [("api_base", "API Base:", true, PromptKind::String)];

    fn request_builder(&self, client: &ReqwestClient, data: SendData) -> Result<RequestBuilder> {
        let api_base = self.get_api_base()?;

        let func = match data.stream {
            true => "streamGenerateContent",
            false => "generateContent",
        };
        let url = format!("{api_base}/{}:{}", &self.model.name, func);

        let block_threshold = self.config.block_threshold.clone();
        let body = gemini_build_body(data, &self.model, block_threshold)?;

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

    async fn send_message_inner(&self, client: &ReqwestClient, data: SendData) -> Result<String> {
        self.prepare_access_token().await?;
        let builder = self.request_builder(client, data)?;
        gemini_send_message(builder).await
    }

    async fn send_message_streaming_inner(
        &self,
        client: &ReqwestClient,
        handler: &mut ReplyHandler,
        data: SendData,
    ) -> Result<()> {
        self.prepare_access_token().await?;
        let builder = self.request_builder(client, data)?;
        gemini_send_message_streaming(builder, handler).await
    }
}

pub async fn gemini_send_message(builder: RequestBuilder) -> Result<String> {
    let res = builder.send().await?;
    let status = res.status();
    let data: Value = res.json().await?;
    if status != 200 {
        catch_error(&data, status.as_u16())?;
    }
    let output = gemini_extract_text(&data)?;
    Ok(output.to_string())
}

pub async fn gemini_send_message_streaming(
    builder: RequestBuilder,
    handler: &mut ReplyHandler,
) -> Result<()> {
    let res = builder.send().await?;
    let status = res.status();
    if status != 200 {
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
