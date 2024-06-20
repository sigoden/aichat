use super::*;

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

const CHAT_COMPLETIONS_API_URL: &str =
    "https://dashscope.aliyuncs.com/api/v1/services/aigc/text-generation/generation";

const CHAT_COMPLETIONS_API_URL_VL: &str =
    "https://dashscope.aliyuncs.com/api/v1/services/aigc/multimodal-generation/generation";

const EMBEDDINGS_API_URL: &str =
    "https://dashscope.aliyuncs.com/api/v1/services/embeddings/text-embedding/text-embedding";

#[derive(Debug, Clone, Deserialize, Default)]
pub struct QianwenConfig {
    pub name: Option<String>,
    pub api_key: Option<String>,
    #[serde(default)]
    pub models: Vec<ModelData>,
    pub patches: Option<ModelPatches>,
    pub extra: Option<ExtraConfig>,
}

impl QianwenClient {
    config_get_fn!(api_key, get_api_key);

    pub const PROMPTS: [PromptAction<'static>; 1] =
        [("api_key", "API Key:", true, PromptKind::String)];

    fn chat_completions_builder(
        &self,
        client: &ReqwestClient,
        data: ChatCompletionsData,
    ) -> Result<RequestBuilder> {
        let api_key = self.get_api_key()?;

        let stream = data.stream;

        let url = match self.model.supports_vision() {
            true => CHAT_COMPLETIONS_API_URL_VL,
            false => CHAT_COMPLETIONS_API_URL,
        };
        let (mut body, has_upload) = build_chat_completions_body(data, &self.model)?;
        self.patch_chat_completions_body(&mut body);

        debug!("Qianwen Chat Completions Request: {url} {body}");

        let mut builder = client.post(url).bearer_auth(api_key).json(&body);
        if stream {
            builder = builder.header("X-DashScope-SSE", "enable");
        }
        if has_upload {
            builder = builder.header("X-DashScope-OssResourceResolve", "enable");
        }

        Ok(builder)
    }

    fn embeddings_builder(
        &self,
        client: &ReqwestClient,
        data: EmbeddingsData,
    ) -> Result<RequestBuilder> {
        let api_key = self.get_api_key()?;

        let text_type = match data.query {
            true => "query",
            false => "document",
        };

        let body = json!({
            "model": self.model.name(),
            "input": {
                "texts": data.texts,
            },
            "parameters": {
                "text_type": text_type,
            }
        });

        let url = EMBEDDINGS_API_URL;

        debug!("Qianwen Embeddings Request: {url} {body}");

        let builder = client.post(url).bearer_auth(api_key).json(&body);

        Ok(builder)
    }
}

#[async_trait]
impl Client for QianwenClient {
    client_common_fns!();

    async fn chat_completions_inner(
        &self,
        client: &ReqwestClient,
        mut data: ChatCompletionsData,
    ) -> Result<ChatCompletionsOutput> {
        let api_key = self.get_api_key()?;
        patch_messages(self.model.name(), &api_key, &mut data.messages).await?;
        let builder = self.chat_completions_builder(client, data)?;
        chat_completions(builder, &self.model).await
    }

    async fn chat_completions_streaming_inner(
        &self,
        client: &ReqwestClient,
        handler: &mut SseHandler,
        mut data: ChatCompletionsData,
    ) -> Result<()> {
        let api_key = self.get_api_key()?;
        patch_messages(self.model.name(), &api_key, &mut data.messages).await?;
        let builder = self.chat_completions_builder(client, data)?;
        chat_completions_streaming(builder, handler, &self.model).await
    }

    async fn embeddings_inner(
        &self,
        client: &ReqwestClient,
        data: EmbeddingsData,
    ) -> Result<Vec<Vec<f32>>> {
        let builder = self.embeddings_builder(client, data)?;
        embeddings(builder).await
    }
}

async fn chat_completions(builder: RequestBuilder, model: &Model) -> Result<ChatCompletionsOutput> {
    let data: Value = builder.send().await?.json().await?;
    maybe_catch_error(&data)?;

    debug!("non-stream-data: {data}");
    extract_chat_completions_text(&data, model)
}

async fn chat_completions_streaming(
    builder: RequestBuilder,
    handler: &mut SseHandler,
    model: &Model,
) -> Result<()> {
    let model_name = model.name();
    let mut prev_text = String::new();
    let handle = |message: SseMmessage| -> Result<bool> {
        let data: Value = serde_json::from_str(&message.data)?;
        maybe_catch_error(&data)?;
        debug!("stream-data: {data}");
        if model_name == "qwen-long" {
            if let Some(text) = data["output"]["choices"][0]["message"]["content"].as_str() {
                let delta_text = &text[prev_text.len()..];
                prev_text = text.to_string();
                handler.text(delta_text)?;
            }
        } else if model.supports_vision() {
            if let Some(text) =
                data["output"]["choices"][0]["message"]["content"][0]["text"].as_str()
            {
                let delta_text = &text[prev_text.len()..];
                prev_text = text.to_string();
                handler.text(delta_text)?;
            }
        } else if let Some(text) = data["output"]["text"].as_str() {
            if let Some(pos) = text.rfind("✿FUNCTION") {
                if pos > prev_text.len() {
                    let delta_text = &text[prev_text.len()..pos];
                    handler.text(delta_text)?;
                }
                prev_text = text.to_string();
                if let Some((name, arguments)) = parse_tool_call(&text[pos..]) {
                    let arguments: Value = arguments
                        .parse()
                        .with_context(|| format!("Invalid function call {name} {arguments}"))?;
                    handler.tool_call(ToolCall::new(name.to_string(), arguments, None))?;
                }
            } else {
                let delta_text = &text[prev_text.len()..];
                prev_text = text.to_string();
                handler.text(delta_text)?;
            }
        }
        Ok(false)
    };

    sse_stream(builder, handle).await
}

fn build_chat_completions_body(data: ChatCompletionsData, model: &Model) -> Result<(Value, bool)> {
    let ChatCompletionsData {
        messages,
        temperature,
        top_p,
        functions,
        stream: _,
    } = data;

    let mut has_upload = false;
    let input = if model.supports_vision() {
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
                    MessageContent::ToolResults(_) => {
                        vec![]
                    }
                };
                json!({ "role": role, "content": content })
            })
            .collect();

        json!({
            "messages": messages,
        })
    } else {
        let messages: Vec<Value> = messages
            .into_iter()
            .flat_map(|message| {
                let role = message.role;
                match message.content {
                    MessageContent::Text(text) => vec![json!({ "role": role, "content": text })],
                    MessageContent::Array(list) => {
                        let parts: Vec<_> = list
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
                            .collect();
                        vec![json!({ "role": role, "content": parts })]
                    }
                    MessageContent::ToolResults((tool_results, _)) => {
                        let content = tool_results
                            .iter()
                            .map(|tool_result| {
                                format!(
                                    "✿FUNCTION✿: {}\n✿ARGS✿: {}\n✿RESULT✿",
                                    tool_result.call.name, tool_result.call.arguments
                                )
                            })
                            .collect::<Vec<String>>()
                            .join("\n");
                        let mut messages =
                            vec![json!({ "role": MessageRole::Assistant, "content": content })];
                        for tool_result in tool_results {
                            messages.push(json!({
                                "role": "tool",
                                "content": tool_result.output.to_string(),
                                "name": tool_result.call.name,
                            }));
                        }
                        messages
                    }
                }
            })
            .collect();
        json!({
            "messages": messages,
        })
    };

    let mut parameters = json!({});

    if let Some(v) = model.max_tokens_param() {
        parameters["max_tokens"] = v.into();
    }
    if let Some(v) = temperature {
        parameters["temperature"] = v.into();
    }
    if let Some(v) = top_p {
        parameters["top_p"] = v.into();
    }

    if let Some(functions) = functions {
        parameters["tools"] = functions
            .iter()
            .map(|v| {
                json!({
                    "type": "function",
                    "function": v,
                })
            })
            .collect();
    }

    let body = json!({
        "model": &model.name(),
        "input": input,
        "parameters": parameters
    });

    Ok((body, has_upload))
}

async fn embeddings(builder: RequestBuilder) -> Result<EmbeddingsOutput> {
    let data: Value = builder.send().await?.json().await?;
    maybe_catch_error(&data)?;
    let res_body: EmbeddingsResBody =
        serde_json::from_value(data).context("Invalid embeddings data")?;
    let output = res_body
        .output
        .embeddings
        .into_iter()
        .map(|v| v.embedding)
        .collect();
    Ok(output)
}

#[derive(Deserialize)]
struct EmbeddingsResBody {
    output: EmbeddingsResBodyOutput,
}

#[derive(Deserialize)]
struct EmbeddingsResBodyOutput {
    embeddings: Vec<EmbeddingsResBodyOutputEmbedding>,
}

#[derive(Deserialize)]
struct EmbeddingsResBodyOutputEmbedding {
    embedding: Vec<f32>,
}

fn extract_chat_completions_text(data: &Value, model: &Model) -> Result<ChatCompletionsOutput> {
    let err = || anyhow!("Invalid response data: {data}");
    let mut tool_calls = vec![];
    let text = if model.name() == "qwen-long" {
        data["output"]["choices"][0]["message"]["content"]
            .as_str()
            .ok_or_else(err)?
    } else if model.supports_vision() {
        data["output"]["choices"][0]["message"]["content"][0]["text"]
            .as_str()
            .ok_or_else(err)?
    } else {
        let text = data["output"]["text"].as_str().ok_or_else(err)?;
        match parse_tool_call(text) {
            Some((name, arguments)) => {
                let arguments: Value = arguments
                    .parse()
                    .with_context(|| format!("Invalid function call {name} {arguments}"))?;
                tool_calls.push(ToolCall::new(name.to_string(), arguments, None));
                ""
            }
            None => text,
        }
    };
    let output = ChatCompletionsOutput {
        text: text.to_string(),
        tool_calls,
        id: data["request_id"].as_str().map(|v| v.to_string()),
        input_tokens: data["usage"]["input_tokens"].as_u64(),
        output_tokens: data["usage"]["output_tokens"].as_u64(),
    };

    Ok(output)
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

fn parse_tool_call(text: &str) -> Option<(&str, &str)> {
    let function_symbol = "✿FUNCTION✿: ";
    let result_symbol = "\n✿RESULT✿: ";
    let args_symbol = "\n✿ARGS✿: ";
    let start = text.find(function_symbol)? + function_symbol.len();
    let text = &text[start..];
    let end = text.find(result_symbol)?;
    let text = &text[..end];
    text.split_once(args_symbol)
}
