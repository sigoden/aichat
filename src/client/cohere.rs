use super::{
    catch_error, extract_system_message, json_stream, message::*, Client, CohereClient,
    CompletionData, CompletionOutput, ExtraConfig, Model, ModelData, ModelPatches, PromptAction,
    PromptKind, SseHandler, ToolCall,
};

use anyhow::{bail, Result};
use reqwest::{Client as ReqwestClient, RequestBuilder};
use serde::Deserialize;
use serde_json::{json, Value};

const API_URL: &str = "https://api.cohere.ai/v1/chat";

#[derive(Debug, Clone, Deserialize, Default)]
pub struct CohereConfig {
    pub name: Option<String>,
    pub api_key: Option<String>,
    #[serde(default)]
    pub models: Vec<ModelData>,
    pub patches: Option<ModelPatches>,
    pub extra: Option<ExtraConfig>,
}

impl CohereClient {
    config_get_fn!(api_key, get_api_key);

    pub const PROMPTS: [PromptAction<'static>; 1] =
        [("api_key", "API Key:", true, PromptKind::String)];

    fn request_builder(
        &self,
        client: &ReqwestClient,
        data: CompletionData,
    ) -> Result<RequestBuilder> {
        let api_key = self.get_api_key()?;

        let mut body = build_body(data, &self.model)?;
        self.patch_request_body(&mut body);

        let url = API_URL;

        debug!("Cohere Request: {url} {body}");

        let builder = client.post(url).bearer_auth(api_key).json(&body);

        Ok(builder)
    }
}

impl_client_trait!(CohereClient, send_message, send_message_streaming);

async fn send_message(builder: RequestBuilder) -> Result<CompletionOutput> {
    let res = builder.send().await?;
    let status = res.status();
    let data: Value = res.json().await?;
    if !status.is_success() {
        catch_error(&data, status.as_u16())?;
    }

    debug!("non-stream-data: {data}");
    extract_completion(&data)
}

async fn send_message_streaming(builder: RequestBuilder, handler: &mut SseHandler) -> Result<()> {
    let res = builder.send().await?;
    let status = res.status();
    if !status.is_success() {
        let data: Value = res.json().await?;
        catch_error(&data, status.as_u16())?;
    } else {
        let handle = |data: &str| -> Result<()> {
            let data: Value = serde_json::from_str(data)?;
            debug!("stream-data: {data}");
            if let Some("text-generation") = data["event_type"].as_str() {
                if let Some(text) = data["text"].as_str() {
                    handler.text(text)?;
                }
            } else if let Some("tool-calls-generation") = data["event_type"].as_str() {
                if let Some(tool_calls) = data["tool_calls"].as_array() {
                    for call in tool_calls {
                        if let (Some(name), Some(args)) =
                            (call["name"].as_str(), call["parameters"].as_object())
                        {
                            handler.tool_call(ToolCall::new(
                                name.to_string(),
                                json!(args),
                                None,
                            ))?;
                        }
                    }
                }
            }
            Ok(())
        };
        json_stream(res.bytes_stream(), handle).await?;
    }
    Ok(())
}

fn build_body(data: CompletionData, model: &Model) -> Result<Value> {
    let CompletionData {
        mut messages,
        temperature,
        top_p,
        functions,
        stream,
    } = data;

    let system_message = extract_system_message(&mut messages);

    let mut image_urls = vec![];
    let mut tool_results = None;

    let mut messages: Vec<Value> = messages
        .into_iter()
        .filter_map(|message| {
            let Message { role, content } = message;
            let role = match role {
                MessageRole::User => "USER",
                _ => "CHATBOT",
            };
            match content {
                MessageContent::Text(text) => Some(json!({
                    "role": role,
                    "message": text,
                })),
                MessageContent::Array(list) => {
                    let list: Vec<String> = list
                        .into_iter()
                        .filter_map(|item| match item {
                            MessageContentPart::Text { text } => Some(text),
                            MessageContentPart::ImageUrl {
                                image_url: ImageUrl { url },
                            } => {
                                image_urls.push(url.clone());
                                None
                            }
                        })
                        .collect();
                    Some(json!({ "role": role, "message": list.join("\n\n") }))
                }
                MessageContent::ToolResults((tool_call_results, _)) => {
                    tool_results = Some(tool_call_results);
                    None
                }
            }
        })
        .collect();

    if !image_urls.is_empty() {
        bail!("The model does not support images: {:?}", image_urls);
    }
    let message = messages.pop().unwrap();
    let message = message["message"].as_str().unwrap_or_default();

    let mut body = json!({
        "model": &model.name(),
        "message": message,
    });

    if let Some(tool_results) = tool_results {
        let tool_results: Vec<_> = tool_results
            .into_iter()
            .map(|tool_call_result| {
                json!({
                    "call": {
                        "name": tool_call_result.call.name,
                        "parameters": tool_call_result.call.arguments,
                    },
                    "outputs": [
                        tool_call_result.output,
                    ]

                })
            })
            .collect();
        body["tool_results"] = json!(tool_results);
    }

    if let Some(v) = system_message {
        body["preamble"] = v.into();
    }

    if !messages.is_empty() {
        body["chat_history"] = messages.into();
    }

    if let Some(v) = model.max_tokens_param() {
        body["max_tokens"] = v.into();
    }
    if let Some(v) = temperature {
        body["temperature"] = v.into();
    }
    if let Some(v) = top_p {
        body["p"] = v.into();
    }
    if stream {
        body["stream"] = true.into();
    }

    if let Some(functions) = functions {
        body["tools"] = functions
            .iter()
            .map(|v| {
                let required = v.parameters.required.clone().unwrap_or_default();
                let mut parameter_definitions = json!({});
                if let Some(properties) = &v.parameters.properties {
                    for (key, value) in properties {
                        let mut value: Value = json!(value);
                        if value.is_object() && required.iter().any(|x| x == key) {
                            value["required"] = true.into();
                        }
                        parameter_definitions[key] = value;
                    }
                }
                json!({
                    "name": v.name,
                    "description": v.description,
                    "parameter_definitions": parameter_definitions,
                })
            })
            .collect();
    }
    Ok(body)
}

fn extract_completion(data: &Value) -> Result<CompletionOutput> {
    let text = data["text"].as_str().unwrap_or_default();

    let mut tool_calls = vec![];
    if let Some(calls) = data["tool_calls"].as_array() {
        tool_calls = calls
            .iter()
            .filter_map(|call| {
                if let (Some(name), Some(parameters)) =
                    (call["name"].as_str(), call["parameters"].as_object())
                {
                    Some(ToolCall::new(name.to_string(), json!(parameters), None))
                } else {
                    None
                }
            })
            .collect()
    }

    if text.is_empty() && tool_calls.is_empty() {
        bail!("Invalid response data: {data}");
    }
    let output = CompletionOutput {
        text: text.to_string(),
        tool_calls,
        id: data["generation_id"].as_str().map(|v| v.to_string()),
        input_tokens: data["meta"]["billed_units"]["input_tokens"].as_u64(),
        output_tokens: data["meta"]["billed_units"]["output_tokens"].as_u64(),
    };
    Ok(output)
}
