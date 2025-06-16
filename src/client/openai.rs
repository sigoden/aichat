use super::*;

use crate::function::{FunctionDeclaration, ToolCall};
use crate::utils::strip_think_tag;

use anyhow::{bail, Context, Result};
use reqwest::RequestBuilder;
use serde::Deserialize;
use serde_json::{json, Value};

const API_BASE: &str = "https://api.openai.com/v1";

#[derive(Debug, Clone, Deserialize, Default)]
pub struct OpenAIConfig {
    pub name: Option<String>,
    pub api_key: Option<String>,
    pub api_base: Option<String>,
    pub organization_id: Option<String>,
    #[serde(default)]
    pub models: Vec<ModelData>,
    pub patch: Option<RequestPatch>,
    pub extra: Option<ExtraConfig>,
}

impl OpenAIClient {
    config_get_fn!(api_key, get_api_key);
    config_get_fn!(api_base, get_api_base);

    pub const PROMPTS: [PromptAction<'static>; 1] = [("api_key", "API Key", None)];
}

impl_client_trait!(
    OpenAIClient,
    (
        prepare_chat_completions,
        openai_chat_completions,
        openai_chat_completions_streaming
    ),
    (prepare_embeddings, openai_embeddings),
    (noop_prepare_rerank, noop_rerank),
);

fn prepare_chat_completions(
    self_: &OpenAIClient,
    data: ChatCompletionsData,
) -> Result<RequestData> {
    let api_key = self_.get_api_key()?;
    let api_base = self_
        .get_api_base()
        .unwrap_or_else(|_| API_BASE.to_string());

    let url = if self_.model.uses_responses_api() {
        format!("{}/responses", api_base.trim_end_matches('/'))
    } else {
        format!("{}/chat/completions", api_base.trim_end_matches('/'))
    };

    let body = openai_build_chat_completions_body(data, &self_.model);

    let mut request_data = RequestData::new(url, body);

    request_data.bearer_auth(api_key);
    if let Some(organization_id) = &self_.config.organization_id {
        request_data.header("OpenAI-Organization", organization_id);
    }

    Ok(request_data)
}

fn prepare_embeddings(self_: &OpenAIClient, data: &EmbeddingsData) -> Result<RequestData> {
    let api_key = self_.get_api_key()?;
    let api_base = self_
        .get_api_base()
        .unwrap_or_else(|_| API_BASE.to_string());

    let url = format!("{api_base}/embeddings");

    let body = openai_build_embeddings_body(data, &self_.model);

    let mut request_data = RequestData::new(url, body);

    request_data.bearer_auth(api_key);
    if let Some(organization_id) = &self_.config.organization_id {
        request_data.header("OpenAI-Organization", organization_id);
    }

    Ok(request_data)
}

pub async fn openai_chat_completions(
    builder: RequestBuilder,
    _model: &Model,
) -> Result<ChatCompletionsOutput> {
    let res = builder.send().await?;
    let status = res.status();
    let data: Value = res.json().await?;
    if !status.is_success() {
        catch_error(&data, status.as_u16())?;
    }

    debug!("non-stream-data: {data}");
    openai_extract_chat_completions(&data)
}

pub async fn openai_chat_completions_streaming(
    builder: RequestBuilder,
    handler: &mut SseHandler,
    model: &Model,
) -> Result<()> {
    let mut call_id = String::new();
    let mut function_name = String::new();
    let mut function_arguments = String::new();
    let mut function_id = String::new();
    let mut reasoning_state = 0;
    let handle = |message: SseMmessage| -> Result<bool> {
        if message.data == "[DONE]" {
            if !function_name.is_empty() {
                if function_arguments.is_empty() {
                    function_arguments = String::from("{}");
                }
                let arguments: Value = function_arguments.parse().with_context(|| {
                    format!("Tool call '{function_name}' have non-JSON arguments '{function_arguments}'")
                })?;
                handler.tool_call(ToolCall::new(
                    function_name.clone(),
                    arguments,
                    normalize_function_id(&function_id),
                ))?;
            }
            return Ok(true);
        }
        let data: Value = serde_json::from_str(&message.data)?;
        debug!("stream-data: {data}");

        // Handle both API formats
        if model.uses_responses_api() {
            handle_responses_api_stream(&data, handler, &mut reasoning_state)?;
        } else {
            handle_chat_completions_stream(&data, handler, &mut reasoning_state)?;
        }
        if let (Some(function), index, id) = (
            data["choices"][0]["delta"]["tool_calls"][0]["function"].as_object(),
            data["choices"][0]["delta"]["tool_calls"][0]["index"].as_u64(),
            data["choices"][0]["delta"]["tool_calls"][0]["id"]
                .as_str()
                .filter(|v| !v.is_empty()),
        ) {
            if reasoning_state == 1 {
                handler.text("\n</think>\n\n")?;
                reasoning_state = 0;
            }
            let maybe_call_id = format!("{}/{}", id.unwrap_or_default(), index.unwrap_or_default());
            if maybe_call_id != call_id && maybe_call_id.len() >= call_id.len() {
                if !function_name.is_empty() {
                    if function_arguments.is_empty() {
                        function_arguments = String::from("{}");
                    }
                    let arguments: Value = function_arguments.parse().with_context(|| {
                        format!("Tool call '{function_name}' have non-JSON arguments '{function_arguments}'")
                    })?;
                    handler.tool_call(ToolCall::new(
                        function_name.clone(),
                        arguments,
                        normalize_function_id(&function_id),
                    ))?;
                }
                function_name.clear();
                function_arguments.clear();
                function_id.clear();
                call_id = maybe_call_id;
            }
            if let Some(name) = function.get("name").and_then(|v| v.as_str()) {
                if name.starts_with(&function_name) {
                    function_name = name.to_string();
                } else {
                    function_name.push_str(name);
                }
            }
            if let Some(arguments) = function.get("arguments").and_then(|v| v.as_str()) {
                function_arguments.push_str(arguments);
            }
            if let Some(id) = id {
                function_id = id.to_string();
            }
        }
        Ok(false)
    };

    sse_stream(builder, handle).await
}

pub async fn openai_embeddings(
    builder: RequestBuilder,
    _model: &Model,
) -> Result<EmbeddingsOutput> {
    let res = builder.send().await?;
    let status = res.status();
    let data: Value = res.json().await?;
    if !status.is_success() {
        catch_error(&data, status.as_u16())?;
    }
    let res_body: EmbeddingsResBody =
        serde_json::from_value(data).context("Invalid embeddings data")?;
    let output = res_body.data.into_iter().map(|v| v.embedding).collect();
    Ok(output)
}

#[derive(Deserialize)]
struct EmbeddingsResBody {
    data: Vec<EmbeddingsResBodyEmbedding>,
}

#[derive(Deserialize)]
struct EmbeddingsResBodyEmbedding {
    embedding: Vec<f32>,
}

pub fn openai_build_chat_completions_body(data: ChatCompletionsData, model: &Model) -> Value {
    let ChatCompletionsData {
        messages,
        temperature,
        top_p,
        functions,
        stream,
    } = data;

    // Check if model uses Responses API
    if model.uses_responses_api() {
        return build_responses_api_body(messages, model, stream, temperature, top_p, functions);
    }

    let messages_len = messages.len();
    let messages: Vec<Value> = messages
        .into_iter()
        .enumerate()
        .flat_map(|(i, message)| {
            let Message { role, content } = message;
            match content {
                MessageContent::ToolCalls(MessageContentToolCalls {
                        tool_results,
                        text,
                        sequence,
                    }) => {
                    if !sequence {
                        let tool_calls: Vec<_> = tool_results.iter().map(|tool_result| {
                            json!({
                                "id": tool_result.call.id,
                                "type": "function",
                                "function": {
                                    "name": tool_result.call.name,
                                    "arguments": tool_result.call.arguments.to_string(),
                                },
                            })
                        }).collect();
                        let text = if text.is_empty() { Value::Null } else { text.into() };
                        let mut messages = vec![
                            json!({ "role": MessageRole::Assistant, "content": text, "tool_calls": tool_calls })
                        ];
                        for tool_result in tool_results {
                            messages.push(
                                json!({
                                    "role": "tool",
                                    "content": tool_result.output.to_string(),
                                    "tool_call_id": tool_result.call.id,
                                })
                            );
                        }
                        messages
                    } else {
                       tool_results.into_iter().flat_map(|tool_result| {
                            vec![
                                json!({
                                    "role": MessageRole::Assistant,
                                    "content": "",
                                    "tool_calls": [
                                        {
                                            "id": tool_result.call.id,
                                            "type": "function",
                                            "function": {
                                                "name": tool_result.call.name,
                                                "arguments": tool_result.call.arguments.to_string(),
                                            },
                                        }
                                    ]
                                }),
                                json!({
                                    "role": "tool",
                                    "content": tool_result.output.to_string(),
                                    "tool_call_id": tool_result.call.id,
                                })
                            ]

                        }).collect()
                    }
                },
                MessageContent::Text(text) if role.is_assistant() && i != messages_len - 1 => vec![
                    json!({ "role": role, "content": strip_think_tag(&text) }
                )],
                _ => vec![json!({ "role": role, "content": content })]
            }
        })
        .collect();

    let mut body = json!({
        "model": &model.real_name(),
        "messages": messages,
    });

    if let Some(v) = model.max_tokens_param() {
        if model
            .patch()
            .and_then(|v| v.get("body").and_then(|v| v.get("max_tokens")))
            == Some(&Value::Null)
        {
            body["max_completion_tokens"] = v.into();
        } else {
            body["max_tokens"] = v.into();
        }
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
    body
}

pub fn openai_build_embeddings_body(data: &EmbeddingsData, model: &Model) -> Value {
    json!({
        "input": data.texts,
        "model": model.real_name()
    })
}

pub fn openai_extract_chat_completions(data: &Value) -> Result<ChatCompletionsOutput> {
    // Check if this is a Responses API response (has "output" field instead of "choices")
    if data.get("output").is_some() {
        return extract_responses_api_output(data);
    }

    let text = data["choices"][0]["message"]["content"]
        .as_str()
        .unwrap_or_default();

    let reasoning = data["choices"][0]["message"]["reasoning_content"]
        .as_str()
        .or_else(|| data["choices"][0]["message"]["reasoning"].as_str())
        .unwrap_or_default()
        .trim();

    let mut tool_calls = vec![];
    if let Some(calls) = data["choices"][0]["message"]["tool_calls"].as_array() {
        for call in calls {
            if let (Some(name), Some(arguments), Some(id)) = (
                call["function"]["name"].as_str(),
                call["function"]["arguments"].as_str(),
                call["id"].as_str(),
            ) {
                let arguments: Value = arguments.parse().with_context(|| {
                    format!("Tool call '{name}' have non-JSON arguments '{arguments}'")
                })?;
                tool_calls.push(ToolCall::new(
                    name.to_string(),
                    arguments,
                    Some(id.to_string()),
                ));
            }
        }
    };

    if text.is_empty() && tool_calls.is_empty() {
        bail!("Invalid response data: {data}");
    }
    let text = if !reasoning.is_empty() {
        format!("<think>\n{reasoning}\n</think>\n\n{text}")
    } else {
        text.to_string()
    };
    let output = ChatCompletionsOutput {
        text,
        tool_calls,
        id: data["id"].as_str().map(|v| v.to_string()),
        input_tokens: data["usage"]["prompt_tokens"].as_u64(),
        output_tokens: data["usage"]["completion_tokens"].as_u64(),
    };
    Ok(output)
}

fn extract_responses_api_output(data: &Value) -> Result<ChatCompletionsOutput> {
    // Extract text from output array
    let mut text = String::new();
    let mut reasoning = String::new();
    let mut tool_calls = vec![];

    if let Some(output_array) = data["output"].as_array() {
        for output_item in output_array {
            if output_item["type"] == "message" {
                if let Some(content_array) = output_item["content"].as_array() {
                    for content_item in content_array {
                        match content_item["type"].as_str() {
                            Some("output_text") => {
                                if let Some(output_text) = content_item["text"].as_str() {
                                    if !text.is_empty() {
                                        text.push('\n');
                                    }
                                    text.push_str(output_text);
                                }
                            }
                            Some("reasoning") => {
                                if let Some(reasoning_text) = content_item["text"].as_str() {
                                    reasoning.push_str(reasoning_text);
                                }
                            }
                            _ => {}
                        }
                    }
                }
            } else if output_item["type"] == "function_call" {
                // Extract function calls from Responses API format
                if let (Some(name), Some(arguments), Some(id)) = (
                    output_item["function"]["name"].as_str(),
                    output_item["function"]["arguments"].as_str(),
                    output_item["id"].as_str(),
                ) {
                    let arguments: Value = arguments.parse().with_context(|| {
                        format!("Tool call '{name}' has non-JSON arguments '{arguments}'")
                    })?;
                    tool_calls.push(ToolCall::new(
                        name.to_string(),
                        arguments,
                        Some(id.to_string()),
                    ));
                }
            }
        }
    }

    if text.is_empty() && tool_calls.is_empty() {
        bail!("Invalid response data: {data}");
    }

    let final_text = if !reasoning.is_empty() {
        format!("<think>\n{reasoning}\n</think>\n\n{text}")
    } else {
        text
    };

    let output = ChatCompletionsOutput {
        text: final_text,
        tool_calls,
        id: data["id"].as_str().map(|v| v.to_string()),
        input_tokens: data["usage"]["input_tokens"].as_u64(),
        output_tokens: data["usage"]["output_tokens"].as_u64(),
    };
    Ok(output)
}

fn build_responses_api_body(
    messages: Vec<Message>,
    model: &Model,
    stream: bool,
    temperature: Option<f64>,
    top_p: Option<f64>,
    functions: Option<Vec<FunctionDeclaration>>
) -> Value {
    // Transform messages to Responses API input format (simple string)
    let input = transform_messages_to_simple_string(&messages);

    let mut body = json!({
        "model": model.real_name(),
        "input": input,
    });

    if stream {
        body["stream"] = true.into();
    }

    if let Some(v) = temperature {
        body["temperature"] = v.into();
    }

    if let Some(v) = top_p {
        body["top_p"] = v.into();
    }

    // Use model's max_tokens_param for max_output_tokens
    if let Some(v) = model.max_tokens_param() {
        body["max_output_tokens"] = v.into();
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

    body
}

fn transform_messages_to_simple_string(messages: &[Message]) -> String {
    // Convert messages to simple string input for Responses API
    messages.iter().map(|message| {
        match &message.content {
            MessageContent::Text(text) => text.clone(),
            MessageContent::Array(parts) => {
                parts.iter().map(|part| {
                    match part {
                        MessageContentPart::Text { text } => text.clone(),
                        MessageContentPart::ImageUrl { .. } => "[Image]".to_string(),
                    }
                }).collect::<Vec<_>>().join(" ")
            }
            MessageContent::ToolCalls(tool_calls) => {
                let mut result = tool_calls.text.clone();
                for tool_result in &tool_calls.tool_results {
                    result.push_str(&format!(" [Tool {}: {}]", tool_result.call.name, tool_result.output));
                }
                result
            }
        }
    }).collect::<Vec<_>>().join("\n\n")
}

fn handle_responses_api_stream(
    data: &Value,
    handler: &mut SseHandler,
    reasoning_state: &mut i32
) -> Result<()> {
    // Handle Responses API streaming format based on event types
    match data["type"].as_str() {
        Some("response.output_text.delta") => {
            // This contains the actual text chunks
            if let Some(delta) = data["delta"].as_str().filter(|v| !v.is_empty()) {
                if *reasoning_state == 1 {
                    handler.text("\n</think>\n\n")?;
                    *reasoning_state = 0;
                }
                handler.text(delta)?;
            }
        }
        Some("response.reasoning.delta") => {
            // Handle reasoning content if present
            if let Some(reasoning) = data["delta"].as_str().filter(|v| !v.is_empty()) {
                if *reasoning_state == 0 {
                    handler.text("<think>\n")?;
                    *reasoning_state = 1;
                }
                handler.text(reasoning)?;
            }
        }
        Some("response.function_call_arguments.delta") => {
            // Handle function call arguments streaming
            if let Some(delta) = data["delta"].as_str().filter(|v| !v.is_empty()) {
                if *reasoning_state == 1 {
                    handler.text("\n</think>\n\n")?;
                    *reasoning_state = 0;
                }
                handler.text(delta)?;
            }
        }
        _ => {
            // Ignore other event types (response.created, response.in_progress, etc.)
        }
    }
    Ok(())
}

fn handle_chat_completions_stream(
    data: &Value,
    handler: &mut SseHandler,
    reasoning_state: &mut i32
) -> Result<()> {
    // Handle standard chat/completions streaming format
    if let Some(text) = data["choices"][0]["delta"]["content"]
        .as_str()
        .filter(|v| !v.is_empty())
    {
        if *reasoning_state == 1 {
            handler.text("\n</think>\n\n")?;
            *reasoning_state = 0;
        }
        handler.text(text)?;
    } else if let Some(text) = data["choices"][0]["delta"]["reasoning_content"]
        .as_str()
        .or_else(|| data["choices"][0]["delta"]["reasoning"].as_str())
        .filter(|v| !v.is_empty())
    {
        if *reasoning_state == 0 {
            handler.text("<think>\n")?;
            *reasoning_state = 1;
        }
        handler.text(text)?;
    }
    Ok(())
}

fn normalize_function_id(value: &str) -> Option<String> {
    if value.is_empty() {
        None
    } else {
        Some(value.to_string())
    }
}
