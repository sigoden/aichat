use super::{
    maybe_catch_error, message::*, sse_stream, Client, CompletionDetails, ExtraConfig, Model,
    ModelConfig, PromptAction, PromptKind, QianwenClient, SendData, SsMmessage, SseHandler,
};

use crate::utils::{base64_decode, sha256};

use anyhow::{anyhow, bail, Context, Result};
use async_trait::async_trait;
use reqwest::{
    multipart::{Form, Part},
    Client as ReqwestClient, RequestBuilder,
};
use serde::Deserialize;
use serde_json::{json, Value};
use std::borrow::BorrowMut;

const API_URL: &str =
    "https://dashscope.aliyuncs.com/api/v1/services/aigc/text-generation/generation";

const API_URL_VL: &str =
    "https://dashscope.aliyuncs.com/api/v1/services/aigc/multimodal-generation/generation";

#[derive(Debug, Clone, Deserialize, Default)]
pub struct QianwenConfig {
    pub name: Option<String>,
    pub api_key: Option<String>,
    #[serde(default)]
    pub models: Vec<ModelConfig>,
    pub extra: Option<ExtraConfig>,
}

impl QianwenClient {
    config_get_fn!(api_key, get_api_key);

    pub const PROMPTS: [PromptAction<'static>; 1] =
        [("api_key", "API Key:", true, PromptKind::String)];

    fn request_builder(&self, client: &ReqwestClient, data: SendData) -> Result<RequestBuilder> {
        let api_key = self.get_api_key()?;

        let stream = data.stream;

        let is_vl = self.is_vl();
        let url = match is_vl {
            true => API_URL_VL,
            false => API_URL,
        };
        let (body, has_upload) = build_body(data, &self.model, is_vl)?;

        debug!("Qianwen Request: {url} {body}");

        let mut builder = client.post(url).bearer_auth(api_key).json(&body);
        if stream {
            builder = builder.header("X-DashScope-SSE", "enable");
        }
        if has_upload {
            builder = builder.header("X-DashScope-OssResourceResolve", "enable");
        }

        Ok(builder)
    }

    fn is_vl(&self) -> bool {
        self.model.name.starts_with("qwen-vl")
    }
}

#[async_trait]
impl Client for QianwenClient {
    client_common_fns!();

    async fn send_message_inner(
        &self,
        client: &ReqwestClient,
        mut data: SendData,
    ) -> Result<(String, CompletionDetails)> {
        let api_key = self.get_api_key()?;
        patch_messages(&self.model.name, &api_key, &mut data.messages).await?;
        let builder = self.request_builder(client, data)?;
        send_message(builder, self.is_vl()).await
    }

    async fn send_message_streaming_inner(
        &self,
        client: &ReqwestClient,
        handler: &mut SseHandler,
        mut data: SendData,
    ) -> Result<()> {
        let api_key = self.get_api_key()?;
        patch_messages(&self.model.name, &api_key, &mut data.messages).await?;
        let builder = self.request_builder(client, data)?;
        send_message_streaming(builder, handler, self.is_vl()).await
    }
}

async fn send_message(builder: RequestBuilder, is_vl: bool) -> Result<(String, CompletionDetails)> {
    let data: Value = builder.send().await?.json().await?;
    maybe_catch_error(&data)?;

    extract_completion_text(&data, is_vl)
}

async fn send_message_streaming(
    builder: RequestBuilder,
    handler: &mut SseHandler,
    is_vl: bool,
) -> Result<()> {
    let handle = |message: SsMmessage| -> Result<bool> {
        let data: Value = serde_json::from_str(&message.data)?;
        maybe_catch_error(&data)?;
        if is_vl {
            if let Some(text) =
                data["output"]["choices"][0]["message"]["content"][0]["text"].as_str()
            {
                handler.text(text)?;
            }
        } else if let Some(text) = data["output"]["text"].as_str() {
            handler.text(text)?;
        }
        Ok(false)
    };

    sse_stream(builder, handle).await
}

fn build_body(data: SendData, model: &Model, is_vl: bool) -> Result<(Value, bool)> {
    let SendData {
        messages,
        temperature,
        top_p,
        stream,
    } = data;

    let mut has_upload = false;
    let input = if is_vl {
        let messages: Vec<Value> = messages
            .into_iter()
            .map(|message| {
                let role = message.role;
                let content = match message.content {
                    MessageContent::Text(text) => vec![json!({"text": text})],
                    MessageContent::Array(list) => list
                        .into_iter()
                        .map(|item| match item {
                            MessageContentPart::Text { text } => json!({"text": text}),
                            MessageContentPart::ImageUrl {
                                image_url: ImageUrl { url },
                            } => {
                                if url.starts_with("oss:") {
                                    has_upload = true;
                                }
                                json!({"image": url})
                            }
                        })
                        .collect(),
                };
                json!({ "role": role, "content": content })
            })
            .collect();

        json!({
            "messages": messages,
        })
    } else {
        json!({
            "messages": messages,
        })
    };

    let mut parameters = json!({});
    if stream {
        parameters["incremental_output"] = true.into();
    }

    if let Some(v) = model.max_tokens_param() {
        parameters["max_tokens"] = v.into();
    }
    if let Some(v) = temperature {
        parameters["temperature"] = v.into();
    }
    if let Some(v) = top_p {
        parameters["top_p"] = v.into();
    }

    let body = json!({
        "model": &model.name,
        "input": input,
        "parameters": parameters
    });

    Ok((body, has_upload))
}

fn extract_completion_text(data: &Value, is_vl: bool) -> Result<(String, CompletionDetails)> {
    let err = || anyhow!("Invalid response data: {data}");
    let text = if is_vl {
        data["output"]["choices"][0]["message"]["content"][0]["text"]
            .as_str()
            .ok_or_else(err)?
    } else {
        data["output"]["text"].as_str().ok_or_else(err)?
    };
    let details = CompletionDetails {
        id: data["request_id"].as_str().map(|v| v.to_string()),
        input_tokens: data["usage"]["input_tokens"].as_u64(),
        output_tokens: data["usage"]["output_tokens"].as_u64(),
    };

    Ok((text.to_string(), details))
}

/// Patch messages, upload embedded images to oss
async fn patch_messages(model: &str, api_key: &str, messages: &mut Vec<Message>) -> Result<()> {
    for message in messages {
        if let MessageContent::Array(list) = message.content.borrow_mut() {
            for item in list {
                if let MessageContentPart::ImageUrl {
                    image_url: ImageUrl { url },
                } = item
                {
                    if url.starts_with("data:") {
                        *url = upload(model, api_key, url)
                            .await
                            .with_context(|| "Failed to upload embedded image to oss")?;
                    }
                }
            }
        }
    }
    Ok(())
}

#[derive(Debug, Deserialize)]
struct Policy {
    data: PolicyData,
}

#[derive(Debug, Deserialize)]
struct PolicyData {
    policy: String,
    signature: String,
    upload_dir: String,
    upload_host: String,
    oss_access_key_id: String,
    x_oss_object_acl: String,
    x_oss_forbid_overwrite: String,
}

/// Upload image to dashscope
async fn upload(model: &str, api_key: &str, url: &str) -> Result<String> {
    let (mime_type, data) = url
        .strip_prefix("data:")
        .and_then(|v| v.split_once(";base64,"))
        .ok_or_else(|| anyhow!("Invalid image url"))?;
    let mut name = sha256(data);
    if let Some(ext) = mime_type.strip_prefix("image/") {
        name.push('.');
        name.push_str(ext);
    }
    let data = base64_decode(data)?;

    let client = reqwest::Client::new();
    let policy: Policy = client
        .get(format!(
            "https://dashscope.aliyuncs.com/api/v1/uploads?action=getPolicy&model={model}"
        ))
        .header("Authorization", format!("Bearer {api_key}"))
        .send()
        .await?
        .json()
        .await?;
    let PolicyData {
        policy,
        signature,
        upload_dir,
        upload_host,
        oss_access_key_id,
        x_oss_object_acl,
        x_oss_forbid_overwrite,
        ..
    } = policy.data;

    let key = format!("{upload_dir}/{name}");
    let file = Part::bytes(data).file_name(name).mime_str(mime_type)?;
    let form = Form::new()
        .text("OSSAccessKeyId", oss_access_key_id)
        .text("Signature", signature)
        .text("policy", policy)
        .text("key", key.clone())
        .text("x-oss-object-acl", x_oss_object_acl)
        .text("x-oss-forbid-overwrite", x_oss_forbid_overwrite)
        .text("success_action_status", "200")
        .text("x-oss-content-type", mime_type.to_string())
        .part("file", file);

    let res = client.post(upload_host).multipart(form).send().await?;

    let status = res.status();
    if !status.is_success() {
        let text = res.text().await?;
        bail!("Invalid response data: {text} (status: {status})")
    }
    Ok(format!("oss://{key}"))
}
