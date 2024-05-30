use super::{
    catch_error, message::*, sse_stream, Client, CompletionData, CompletionOutput, ExtraConfig,
    Model, ModelData, ModelPatches, OpenAIClient, PromptAction, PromptKind, SseHandler,
    SseMmessage, ToolCall,
};

use anyhow::{bail, Result};
use reqwest::{Client as ReqwestClient, RequestBuilder};
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
    pub patches: Option<ModelPatches>,
    pub extra: Option<ExtraConfig>,
}

impl OpenAIClient {
    config_get_fn!(api_key, get_api_key);
    config_get_fn!(api_base, get_api_base);

    pub const PROMPTS: [PromptAction<'static>; 1] =
        [("api_key", "API Key:", true, PromptKind::String)];

    fn request_builder(
        &self,
        client: &ReqwestClient,
        data: CompletionData,
    ) -> Result<RequestBuilder> {
        let api_key = self.get_api_key()?;
        let api_base = self.get_api_base().unwrap_or_else(|_| API_BASE.to_string());

        let mut body = openai_build_body(data, &self.model);
        self.patch_request_body(&mut body);

        let url = format!("{api_base}/chat/completions");

        debug!("OpenAI Request: {url} {body}");

        let mut builder = client.post(url).bearer_auth(api_key).json(&body);

        if let Some(organization_id) = &self.config.organization_id {
            builder = builder.header("OpenAI-Organization", organization_id);
        }

        Ok(builder)
    }
}

pub async fn openai_send_message(builder: RequestBuilder) -> Result<CompletionOutput> {
    let res = builder.send().await?;
    let status = res.status();
    let data: Value = res.json().await?;
    if !status.is_success() {
        catch_error(&data, status.as_u16())?;
    }

    debug!("non-stream-data: {data}");
    openai_extract_completion(&data)
}

pub async fn openai_send_message_streaming(
    builder: RequestBuilder,
    handler: &mut SseHandler,
) -> Result<()> {
    let mut function_index = 0;
    let mut function_name = String::new();
    let mut function_arguments = String::new();
    let mut function_id = String::new();
    let handle = |message: SseMmessage| -> Result<bool> {
        if message.data == "[DONE]" {
            if !function_name.is_empty() {
                handler.tool_call(ToolCall::new(
                    function_name.clone(),
                    json!(function_arguments),
                    Some(function_id.clone()),
                ))?;
            }
            return Ok(true);
        }
        let data: Value = serde_json::from_str(&message.data)?;
        debug!("stream-data: {data}");
        if let Some(text) = data["choices"][0]["delta"]["content"].as_str() {
            handler.text(text)?;
        } else if let (Some(function), index, id) = (
            data["choices"][0]["delta"]["tool_calls"][0]["function"].as_object(),
            data["choices"][0]["delta"]["tool_calls"][0]["index"].as_u64(),
            data["choices"][0]["delta"]["tool_calls"][0]["id"].as_str(),
        ) {
            let index = index.unwrap_or_default();
            if index != function_index {
                if !function_name.is_empty() {
                    handler.tool_call(ToolCall::new(
                        function_name.clone(),
                        json!(function_arguments),
                        Some(function_id.clone()),
                    ))?;
                }
                function_name.clear();
                function_arguments.clear();
                function_id.clear();
                function_index = index;
            }
            if let Some(name) = function.get("name").and_then(|v| v.as_str()) {
                function_name = name.to_string();
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

pub fn openai_build_body(data: CompletionData, model: &Model) -> Value {
    let CompletionData {
        messages,
        temperature,
        top_p,
        functions,
        stream,
    } = data;

    let messages: Vec<Value> = messages
        .into_iter()
        .flat_map(|message| {
            let Message { role, content } = message;
            match content {
                MessageContent::ToolResults((tool_call_results, text)) => {
                    let tool_calls: Vec<_> = tool_call_results.iter().map(|tool_call_result| {
                        json!({
                            "id": tool_call_result.call.id,
                            "type": "function",
                            "function": {
                                "name": tool_call_result.call.name,
                                "arguments": tool_call_result.call.arguments,
                            },
                        })
                    }).collect();
                    let mut messages = vec![
                        json!({ "role": MessageRole::Assistant, "content": text, "tool_calls": tool_calls })
                    ];
                    for tool_call_result in tool_call_results {
                        messages.push(
                            json!({
                                "role": "tool",
                                "content": tool_call_result.output.to_string(),
                                "tool_call_id": tool_call_result.call.id,
                            })
                        );
                    }
                    messages
                },
                _ => vec![json!({ "role": role, "content": content })]
            }
        })
        .collect();

    let mut body = json!({
        "model": &model.name(),
        "messages": messages,
    });

    if let Some(v) = model.max_tokens_param() {
        body["max_tokens"] = v.into();
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
        body["tool_choice"] = "auto".into();
    }
    body
}

pub fn openai_extract_completion(data: &Value) -> Result<CompletionOutput> {
    let text = data["choices"][0]["message"]["content"]
        .as_str()
        .unwrap_or_default();

    let mut tool_calls = vec![];
    if let Some(tools_call) = data["choices"][0]["message"]["tool_calls"].as_array() {
        tool_calls = tools_call
            .iter()
            .filter_map(|call| {
                if let (Some(name), Some(arguments), Some(id)) = (
                    call["function"]["name"].as_str(),
                    call["function"]["arguments"].as_str(),
                    call["id"].as_str(),
                ) {
                    Some(ToolCall::new(
                        name.to_string(),
                        json!(arguments),
                        Some(id.to_string()),
                    ))
                } else {
                    None
                }
            })
            .collect()
    };

    if text.is_empty() && tool_calls.is_empty() {
        bail!("Invalid response data: {data}");
    }
    let output = CompletionOutput {
        text: text.to_string(),
        tool_calls,
        id: data["id"].as_str().map(|v| v.to_string()),
        input_tokens: data["usage"]["prompt_tokens"].as_u64(),
        output_tokens: data["usage"]["completion_tokens"].as_u64(),
    };
    Ok(output)
}

impl_client_trait!(
    OpenAIClient,
    openai_send_message,
    openai_send_message_streaming
);
