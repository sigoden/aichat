use super::*;

use anyhow::{bail, Context, Result};
use reqwest::RequestBuilder;
use serde::Deserialize;
use serde_json::{json, Value};

#[derive(Debug, Clone, Deserialize, Default)]
pub struct OllamaConfig {
    pub name: Option<String>,
    pub api_base: Option<String>,
    pub api_auth: Option<String>,
    #[serde(default)]
    pub models: Vec<ModelData>,
    pub patch: Option<RequestPatch>,
    pub extra: Option<ExtraConfig>,
}

impl OllamaClient {
    config_get_fn!(api_base, get_api_base);
    config_get_fn!(api_auth, get_api_auth);

    pub const PROMPTS: [PromptAction<'static>; 4] = [
        ("api_base", "API Base:", true, PromptKind::String),
        ("api_auth", "API Auth:", false, PromptKind::String),
        ("models[].name", "Model Name:", true, PromptKind::String),
        (
            "models[].max_input_tokens",
            "Max Input Tokens:",
            false,
            PromptKind::Integer,
        ),
    ];
}

impl_client_trait!(
    OllamaClient,
    (
        prepare_chat_completions,
        chat_completions,
        chat_completions_streaming
    ),
    (prepare_embeddings, embeddings),
    (noop_prepare_rerank, noop_rerank),
);

fn prepare_chat_completions(
    self_: &OllamaClient,
    data: ChatCompletionsData,
) -> Result<RequestData> {
    let api_base = self_.get_api_base()?;
    let api_auth = self_.get_api_auth().ok();

    let url = format!("{api_base}/api/chat");

    let body = build_chat_completions_body(data, &self_.model)?;

    let mut request_data = RequestData::new(url, body);

    if let Some(api_auth) = api_auth {
        request_data.header("Authorization", api_auth)
    }

    Ok(request_data)
}

fn prepare_embeddings(self_: &OllamaClient, data: EmbeddingsData) -> Result<RequestData> {
    let api_base = self_.get_api_base()?;
    let api_auth = self_.get_api_auth().ok();

    let url = format!("{api_base}/api/embed");

    let body = json!({
        "model": self_.model.name(),
        "input": data.texts,
    });

    let mut request_data = RequestData::new(url, body);

    if let Some(api_auth) = api_auth {
        request_data.header("Authorization", api_auth)
    }

    Ok(request_data)
}

async fn chat_completions(
    builder: RequestBuilder,
    _model: &Model,
) -> Result<ChatCompletionsOutput> {
    let res = builder.send().await?;
    let status = res.status();
    let data = res.json().await?;
    if !status.is_success() {
        catch_error(&data, status.as_u16())?;
    }
    debug!("non-stream-data: {data}");

    extract_chat_completions(&data)
}

async fn chat_completions_streaming(
    builder: RequestBuilder,
    handler: &mut SseHandler,
    _model: &Model,
) -> Result<()> {
    let res = builder.send().await?;
    let status = res.status();
    if !status.is_success() {
        let data = res.json().await?;
        catch_error(&data, status.as_u16())?;
    } else {
        let handle = |message: &str| -> Result<()> {
            let data: Value = serde_json::from_str(message)?;
            debug!("stream-data: {data}");

            if data["done"].is_boolean() {
                if let Some(text) = data["message"]["content"].as_str() {
                    handler.text(text)?;
                }
            } else {
                bail!("Invalid response data: {data}")
            }

            Ok(())
        };

        json_stream(res.bytes_stream(), handle).await?;
    }

    Ok(())
}

async fn embeddings(builder: RequestBuilder, _model: &Model) -> Result<EmbeddingsOutput> {
    let res = builder.send().await?;
    let status = res.status();
    let data = res.json().await?;
    if !status.is_success() {
        catch_error(&data, status.as_u16())?;
    }
    let res_body: EmbeddingsResBody =
        serde_json::from_value(data).context("Invalid embeddings data")?;
    Ok(res_body.embeddings)
}

#[derive(Deserialize)]
struct EmbeddingsResBody {
    embeddings: Vec<Vec<f32>>,
}

fn build_chat_completions_body(data: ChatCompletionsData, model: &Model) -> Result<Value> {
    let ChatCompletionsData {
        messages,
        temperature,
        top_p,
        functions,
        stream,
    } = data;

    let mut network_image_urls = vec![];

    let messages: Vec<Value> = messages
        .into_iter()
        .flat_map(|message| {
            let Message { role, content } = message;
            match content {
                MessageContent::Text(text) => vec![json!({
                    "role": role,
                    "content": text,
                })],
                MessageContent::Array(list) => {
                    let mut content = vec![];
                    let mut images = vec![];
                    for item in list {
                        match item {
                            MessageContentPart::Text { text } => {
                                content.push(text);
                            }
                            MessageContentPart::ImageUrl {
                                image_url: ImageUrl { url },
                            } => {
                                if let Some((_, data)) = url
                                    .strip_prefix("data:")
                                    .and_then(|v| v.split_once(";base64,"))
                                {
                                    images.push(data.to_string());
                                } else {
                                    network_image_urls.push(url.clone());
                                }
                            }
                        }
                    }
                    let content = content.join("\n\n");
                    vec![json!({ "role": role, "content": content, "images": images })]
                }
                MessageContent::ToolResults((tool_results, text)) => {
                    let tool_calls: Vec<_> = tool_results.iter().map(|tool_result| {
                        json!({
                            "function": {
                                "name": tool_result.call.name,
                                "arguments": tool_result.call.arguments,
                            },
                        })
                    }).collect();
                    let mut messages = vec![
                        json!({ "role": MessageRole::Assistant, "content": text, "tool_calls": tool_calls })
                    ];
                    for tool_result in tool_results {
                        messages.push(
                            json!({
                                "role": "tool",
                                "content": tool_result.output.to_string(),
                            })
                        );
                    }
                    messages
                },
            }
        })
        .collect();

    if !network_image_urls.is_empty() {
        bail!(
            "The model does not support network images: {:?}",
            network_image_urls
        );
    }

    let mut body = json!({
        "model": &model.name(),
        "messages": messages,
        "stream": stream,
        "options": {},
    });

    if let Some(v) = model.max_tokens_param() {
        body["options"]["num_predict"] = v.into();
    }
    if let Some(v) = temperature {
        body["options"]["temperature"] = v.into();
    }
    if let Some(v) = top_p {
        body["options"]["top_p"] = v.into();
    }
    if let Some(functions) = functions {
        body["tools"] = functions
            .iter()
            .map(|v| {
                json!({
                    "type": "function",
                    "function": v,
                })
            })
            .collect();
    }

    Ok(body)
}

fn extract_chat_completions(data: &Value) -> Result<ChatCompletionsOutput> {
    let text = data["message"]["content"].as_str().unwrap_or_default();

    let mut tool_calls = vec![];
    if let Some(calls) = data["message"]["tool_calls"].as_array() {
        tool_calls = calls
            .iter()
            .filter_map(|call| {
                if let (Some(name), arguments) = (
                    call["function"]["name"].as_str(),
                    call["function"]["arguments"].clone(),
                ) {
                    Some(ToolCall::new(name.to_string(), arguments, None))
                } else {
                    None
                }
            })
            .collect()
    };

    if text.is_empty() && tool_calls.is_empty() {
        bail!("Invalid response data: {data}");
    }
    let output = ChatCompletionsOutput {
        text: text.to_string(),
        tool_calls,
        id: None,
        input_tokens: data["prompt_eval_count"].as_u64(),
        output_tokens: data["eval_count"].as_u64(),
    };
    Ok(output)
}
