use super::openai::*;
use super::openai_compatible::*;
use super::*;

use anyhow::{bail, Context, Result};
use reqwest::RequestBuilder;
use serde::Deserialize;
use serde_json::{json, Value};

const API_BASE: &str = "https://api.cohere.ai/v2";

#[derive(Debug, Clone, Deserialize, Default)]
pub struct CohereConfig {
    pub name: Option<String>,
    pub api_key: Option<String>,
    pub api_base: Option<String>,
    #[serde(default)]
    pub models: Vec<ModelData>,
    pub patch: Option<RequestPatch>,
    pub extra: Option<ExtraConfig>,
}

impl CohereClient {
    config_get_fn!(api_key, get_api_key);
    config_get_fn!(api_base, get_api_base);

    pub const PROMPTS: [PromptAction<'static>; 1] = [("api_key", "API Key", None)];
}

impl_client_trait!(
    CohereClient,
    (
        prepare_chat_completions,
        chat_completions,
        chat_completions_streaming
    ),
    (prepare_embeddings, embeddings),
    (prepare_rerank, generic_rerank),
);

fn prepare_chat_completions(
    self_: &CohereClient,
    data: ChatCompletionsData,
) -> Result<RequestData> {
    let api_key = self_.get_api_key()?;
    let api_base = self_
        .get_api_base()
        .unwrap_or_else(|_| API_BASE.to_string());

    let url = format!("{}/chat", api_base.trim_end_matches('/'));
    let mut body = openai_build_chat_completions_body(data, &self_.model);
    if let Some(obj) = body.as_object_mut() {
        if let Some(top_p) = obj.remove("top_p") {
            obj.insert("p".to_string(), top_p);
        }
    }

    let mut request_data = RequestData::new(url, body);

    request_data.bearer_auth(api_key);

    Ok(request_data)
}

fn prepare_embeddings(self_: &CohereClient, data: &EmbeddingsData) -> Result<RequestData> {
    let api_key = self_.get_api_key()?;
    let api_base = self_
        .get_api_base()
        .unwrap_or_else(|_| API_BASE.to_string());

    let url = format!("{}/embed", api_base.trim_end_matches('/'));

    let input_type = match data.query {
        true => "search_query",
        false => "search_document",
    };

    let body = json!({
        "model": self_.model.real_name(),
        "texts": data.texts,
        "input_type": input_type,
        "embedding_types": ["float"],
    });

    let mut request_data = RequestData::new(url, body);

    request_data.bearer_auth(api_key);

    Ok(request_data)
}

fn prepare_rerank(self_: &CohereClient, data: &RerankData) -> Result<RequestData> {
    let api_key = self_.get_api_key()?;
    let api_base = self_
        .get_api_base()
        .unwrap_or_else(|_| API_BASE.to_string());

    let url = format!("{}/rerank", api_base.trim_end_matches('/'));
    let body = generic_build_rerank_body(data, &self_.model);

    let mut request_data = RequestData::new(url, body);

    request_data.bearer_auth(api_key);

    Ok(request_data)
}

async fn chat_completions(
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
    extract_chat_completions(&data)
}

async fn chat_completions_streaming(
    builder: RequestBuilder,
    handler: &mut SseHandler,
    _model: &Model,
) -> Result<()> {
    let mut function_name = String::new();
    let mut function_arguments = String::new();
    let mut function_id = String::new();
    let handle = |message: SseMmessage| -> Result<bool> {
        if message.data == "[DONE]" {
            return Ok(true);
        }
        let data: Value = serde_json::from_str(&message.data)?;
        debug!("stream-data: {data}");
        if let Some(typ) = data["type"].as_str() {
            match typ {
                "content-delta" => {
                    if let Some(text) = data["delta"]["message"]["content"]["text"].as_str() {
                        handler.text(text)?;
                    }
                }
                "tool-plan-delta" => {
                    if let Some(text) = data["delta"]["message"]["tool_plan"].as_str() {
                        handler.text(text)?;
                    }
                }
                "tool-call-start" => {
                    if let (Some(function), Some(id)) = (
                        data["delta"]["message"]["tool_calls"]["function"].as_object(),
                        data["delta"]["message"]["tool_calls"]["id"].as_str(),
                    ) {
                        if let Some(name) = function.get("name").and_then(|v| v.as_str()) {
                            function_name = name.to_string();
                        }
                        function_id = id.to_string();
                    }
                }
                "tool-call-delta" => {
                    if let Some(text) =
                        data["delta"]["message"]["tool_calls"]["function"]["arguments"].as_str()
                    {
                        function_arguments.push_str(text);
                    }
                }
                "tool-call-end" => {
                    if !function_name.is_empty() {
                        let arguments: Value = function_arguments.parse().with_context(|| {
                            format!("Tool call '{function_name}' have non-JSON arguments '{function_arguments}'")
                        })?;
                        handler.tool_call(ToolCall::new(
                            function_name.clone(),
                            arguments,
                            Some(function_id.clone()),
                        ))?;
                    }
                    function_name.clear();
                    function_arguments.clear();
                    function_id.clear();
                }
                _ => {}
            }
        }
        Ok(false)
    };

    sse_stream(builder, handle).await
}

async fn embeddings(builder: RequestBuilder, _model: &Model) -> Result<EmbeddingsOutput> {
    let res = builder.send().await?;
    let status = res.status();
    let data: Value = res.json().await?;
    if !status.is_success() {
        catch_error(&data, status.as_u16())?;
    }
    let res_body: EmbeddingsResBody =
        serde_json::from_value(data).context("Invalid embeddings data")?;
    Ok(res_body.embeddings.float)
}

#[derive(Deserialize)]
struct EmbeddingsResBody {
    embeddings: EmbeddingsResBodyEmbeddings,
}

#[derive(Deserialize)]
struct EmbeddingsResBodyEmbeddings {
    float: Vec<Vec<f32>>,
}

fn extract_chat_completions(data: &Value) -> Result<ChatCompletionsOutput> {
    let mut text = data["message"]["content"][0]["text"]
        .as_str()
        .unwrap_or_default()
        .to_string();

    let mut tool_calls = vec![];
    if let Some(calls) = data["message"]["tool_calls"].as_array() {
        if text.is_empty() {
            if let Some(tool_plain) = data["message"]["tool_plan"].as_str() {
                text = tool_plain.to_string();
            }
        }
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
    }

    if text.is_empty() && tool_calls.is_empty() {
        bail!("Invalid response data: {data}");
    }
    let output = ChatCompletionsOutput {
        text,
        tool_calls,
        id: data["id"].as_str().map(|v| v.to_string()),
        input_tokens: data["usage"]["billed_units"]["input_tokens"].as_u64(),
        output_tokens: data["usage"]["billed_units"]["output_tokens"].as_u64(),
    };
    Ok(output)
}
