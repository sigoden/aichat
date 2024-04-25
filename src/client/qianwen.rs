use super::{
    maybe_catch_error, message::*, Client, ExtraConfig, Model, ModelConfig, PromptType,
    QianwenClient, ReplyHandler, SendData,
};

use crate::utils::{sha256sum, PromptKind};

use anyhow::{anyhow, bail, Context, Result};
use async_trait::async_trait;
use base64::{engine::general_purpose::STANDARD, Engine};
use futures_util::StreamExt;
use reqwest::{
    multipart::{Form, Part},
    Client as ReqwestClient, RequestBuilder,
};
use reqwest_eventsource::{Error as EventSourceError, Event, RequestBuilderExt};
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
    list_models_fn!(
        QianwenConfig,
        [
            // https://help.aliyun.com/zh/dashscope/developer-reference/api-details
            ("qwen-turbo", "text", 6000),
            ("qwen-plus", "text", 30000),
            ("qwen-max", "text", 6000),
            ("qwen-max-longcontext", "text", 28000),
            // https://help.aliyun.com/zh/dashscope/developer-reference/tongyi-qianwen-vl-plus-api
            ("qwen-vl-plus", "text,vision", 0),
            ("qwen-vl-max", "text,vision", 0),
        ]
    );
    config_get_fn!(api_key, get_api_key);

    pub const PROMPTS: [PromptType<'static>; 1] =
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

async fn send_message(builder: RequestBuilder, is_vl: bool) -> Result<String> {
    let data: Value = builder.send().await?.json().await?;
    maybe_catch_error(&data)?;

    let output = if is_vl {
        data["output"]["choices"][0]["message"]["content"][0]["text"].as_str()
    } else {
        data["output"]["text"].as_str()
    };

    let output = output.ok_or_else(|| anyhow!("Unexpected response {data}"))?;

    Ok(output.to_string())
}

async fn send_message_streaming(
    builder: RequestBuilder,
    handler: &mut ReplyHandler,
    is_vl: bool,
) -> Result<()> {
    let mut es = builder.eventsource()?;

    while let Some(event) = es.next().await {
        match event {
            Ok(Event::Open) => {}
            Ok(Event::Message(message)) => {
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
            }
            Err(err) => {
                match err {
                    EventSourceError::StreamEnded => {}
                    _ => {
                        bail!("{}", err);
                    }
                }
                es.close();
            }
        }
    }

    Ok(())
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

    if let Some(v) = model.max_output_tokens {
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

/// Patch messsages, upload embedded images to oss
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
    let mut name = sha256sum(data);
    if let Some(ext) = mime_type.strip_prefix("image/") {
        name.push('.');
        name.push_str(ext);
    }
    let data = STANDARD.decode(data)?;

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
    if res.status() != 200 {
        let text = res.text().await?;
        bail!("Invalid response data: {text} (status: {status})")
    }
    Ok(format!("oss://{key}"))
}

#[async_trait]
impl Client for QianwenClient {
    client_common_fns!();

    async fn send_message_inner(
        &self,
        client: &ReqwestClient,
        mut data: SendData,
    ) -> Result<String> {
        let api_key = self.get_api_key()?;
        patch_messages(&self.model.name, &api_key, &mut data.messages).await?;
        let builder = self.request_builder(client, data)?;
        send_message(builder, self.is_vl()).await
    }

    async fn send_message_streaming_inner(
        &self,
        client: &ReqwestClient,
        handler: &mut ReplyHandler,
        mut data: SendData,
    ) -> Result<()> {
        let api_key = self.get_api_key()?;
        patch_messages(&self.model.name, &api_key, &mut data.messages).await?;
        let builder = self.request_builder(client, data)?;
        send_message_streaming(builder, handler, self.is_vl()).await
    }
}
