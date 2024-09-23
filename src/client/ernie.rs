use super::access_token::*;
use super::openai_compatible::*;
use super::*;

use anyhow::{anyhow, bail, Context, Result};
use reqwest::{Client as ReqwestClient, RequestBuilder};
use serde::Deserialize;
use serde_json::{json, Value};

const API_BASE: &str = "https://aip.baidubce.com/rpc/2.0/ai_custom/v1";
const ACCESS_TOKEN_URL: &str = "https://aip.baidubce.com/oauth/2.0/token";

#[derive(Debug, Clone, Deserialize, Default)]
pub struct ErnieConfig {
    pub name: Option<String>,
    pub api_key: Option<String>,
    pub secret_key: Option<String>,
    #[serde(default)]
    pub models: Vec<ModelData>,
    pub patch: Option<RequestPatch>,
    pub extra: Option<ExtraConfig>,
}

impl ErnieClient {
    config_get_fn!(api_key, get_api_key);
    config_get_fn!(secret_key, get_secret_key);
    pub const PROMPTS: [PromptAction<'static>; 2] = [
        ("api_key", "API Key:", true, PromptKind::String),
        ("secret_key", "Secret Key:", true, PromptKind::String),
    ];
}

#[async_trait::async_trait]
impl Client for ErnieClient {
    client_common_fns!();

    async fn chat_completions_inner(
        &self,
        client: &ReqwestClient,
        data: ChatCompletionsData,
    ) -> Result<ChatCompletionsOutput> {
        prepare_access_token(self, client).await?;
        let request_data = prepare_chat_completions(self, data)?;
        let builder = self.request_builder(client, request_data, ApiType::ChatCompletions);
        chat_completions(builder, &self.model).await
    }

    async fn chat_completions_streaming_inner(
        &self,
        client: &ReqwestClient,
        handler: &mut SseHandler,
        data: ChatCompletionsData,
    ) -> Result<()> {
        prepare_access_token(self, client).await?;
        let request_data = prepare_chat_completions(self, data)?;
        let builder = self.request_builder(client, request_data, ApiType::ChatCompletions);
        chat_completions_streaming(builder, handler, &self.model).await
    }

    async fn embeddings_inner(
        &self,
        client: &ReqwestClient,
        data: &EmbeddingsData,
    ) -> Result<EmbeddingsOutput> {
        prepare_access_token(self, client).await?;
        let request_data = prepare_embeddings(self, data)?;
        let builder = self.request_builder(client, request_data, ApiType::Embeddings);
        embeddings(builder, &self.model).await
    }

    async fn rerank_inner(
        &self,
        client: &ReqwestClient,
        data: &RerankData,
    ) -> Result<RerankOutput> {
        prepare_access_token(self, client).await?;
        let request_data = prepare_rerank(self, data)?;
        let builder = self.request_builder(client, request_data, ApiType::Rerank);
        rerank(builder, &self.model).await
    }
}

fn prepare_chat_completions(self_: &ErnieClient, data: ChatCompletionsData) -> Result<RequestData> {
    let access_token = get_access_token(self_.name())?;

    let url = format!(
        "{API_BASE}/wenxinworkshop/chat/{}?access_token={access_token}",
        self_.model.name(),
    );

    let body = build_chat_completions_body(data, &self_.model);

    let request_data = RequestData::new(url, body);

    Ok(request_data)
}

fn prepare_embeddings(self_: &ErnieClient, data: &EmbeddingsData) -> Result<RequestData> {
    let access_token = get_access_token(self_.name())?;

    let url = format!(
        "{API_BASE}/wenxinworkshop/embeddings/{}?access_token={access_token}",
        self_.model.name(),
    );

    let body = json!({
        "input": data.texts,
    });

    let request_data = RequestData::new(url, body);

    Ok(request_data)
}

fn prepare_rerank(self_: &ErnieClient, data: &RerankData) -> Result<RequestData> {
    let access_token = get_access_token(self_.name())?;

    let url = format!(
        "{API_BASE}/wenxinworkshop/reranker/{}?access_token={access_token}",
        self_.model.name(),
    );

    let RerankData {
        query,
        documents,
        top_n,
    } = data;

    let body = json!({
        "query": query,
        "documents": documents,
        "top_n": top_n
    });

    let request_data = RequestData::new(url, body);

    Ok(request_data)
}

async fn prepare_access_token(self_: &ErnieClient, client: &ReqwestClient) -> Result<()> {
    let client_name = self_.name();
    if !is_valid_access_token(client_name) {
        let api_key = self_.get_api_key()?;
        let secret_key = self_.get_secret_key()?;

        let token = fetch_access_token(client, &api_key, &secret_key)
            .await
            .with_context(|| "Failed to fetch access token")?;
        set_access_token(client_name, token, 86400);
    }
    Ok(())
}

async fn chat_completions(
    builder: RequestBuilder,
    _model: &Model,
) -> Result<ChatCompletionsOutput> {
    let data: Value = builder.send().await?.json().await?;
    maybe_catch_error(&data)?;
    debug!("non-stream-data: {data}");
    extract_chat_completions_text(&data)
}

async fn chat_completions_streaming(
    builder: RequestBuilder,
    handler: &mut SseHandler,
    _model: &Model,
) -> Result<()> {
    let handle = |message: SseMmessage| -> Result<bool> {
        let data: Value = serde_json::from_str(&message.data)?;
        debug!("stream-data: {data}");
        if let Some(function) = data["function_call"].as_object() {
            if let (Some(name), Some(arguments)) = (
                function.get("name").and_then(|v| v.as_str()),
                function.get("arguments").and_then(|v| v.as_str()),
            ) {
                let arguments: Value = arguments.parse().with_context(|| {
                    format!("Tool call '{name}' is invalid: arguments must be in valid JSON format")
                })?;
                handler.tool_call(ToolCall::new(name.to_string(), arguments, None))?;
            }
        } else if let Some(text) = data["result"].as_str() {
            handler.text(text)?;
        }
        Ok(false)
    };

    sse_stream(builder, handle).await
}

async fn embeddings(builder: RequestBuilder, _model: &Model) -> Result<EmbeddingsOutput> {
    let data: Value = builder.send().await?.json().await?;
    maybe_catch_error(&data)?;
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

async fn rerank(builder: RequestBuilder, _model: &Model) -> Result<RerankOutput> {
    let data: Value = builder.send().await?.json().await?;
    maybe_catch_error(&data)?;
    let res_body: GenericRerankResBody =
        serde_json::from_value(data).context("Invalid rerank data")?;
    Ok(res_body.results)
}

fn build_chat_completions_body(data: ChatCompletionsData, model: &Model) -> Value {
    let ChatCompletionsData {
        mut messages,
        temperature,
        top_p,
        functions,
        stream,
    } = data;

    let system_message = extract_system_message(&mut messages);

    let messages: Vec<Value> = messages
        .into_iter()
        .flat_map(|message| {
            let Message { role, content } = message;
            match content {
                MessageContent::ToolResults((tool_results, _)) => {
                    let mut list = vec![];
                    for tool_result in tool_results {
                        list.push(json!({
                            "role": "assistant",
                            "content": format!("Action: {}\nAction Input: {}", tool_result.call.name, tool_result.call.arguments)
                        }));
                        list.push(json!({
                            "role": "user",
                            "content": tool_result.output.to_string(),
                        }))

                    }
                    list
                }
                _ => vec![json!({ "role": role, "content": content })],
            }
        })
        .collect();

    let mut body = json!({
        "messages": messages,
    });

    if let Some(v) = system_message {
        body["system"] = v.into();
    }

    if let Some(v) = model.max_tokens_param() {
        body["max_output_tokens"] = v.into();
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
        body["functions"] = json!(functions);
    }

    body
}

fn extract_chat_completions_text(data: &Value) -> Result<ChatCompletionsOutput> {
    let text = data["result"].as_str().unwrap_or_default();

    let mut tool_calls = vec![];
    if let Some(call) = data["function_call"].as_object() {
        if let (Some(name), Some(arguments)) = (
            call.get("name").and_then(|v| v.as_str()),
            call.get("arguments").and_then(|v| v.as_str()),
        ) {
            let arguments: Value = arguments.parse().with_context(|| {
                format!("Tool call '{name}' is invalid: arguments must be in valid JSON format")
            })?;
            tool_calls.push(ToolCall::new(name.to_string(), arguments, None));
        }
    }

    if text.is_empty() && tool_calls.is_empty() {
        bail!("Invalid response data: {data}");
    }
    let output = ChatCompletionsOutput {
        text: text.to_string(),
        tool_calls,
        id: data["id"].as_str().map(|v| v.to_string()),
        input_tokens: data["usage"]["prompt_tokens"].as_u64(),
        output_tokens: data["usage"]["completion_tokens"].as_u64(),
    };
    Ok(output)
}

async fn fetch_access_token(
    client: &reqwest::Client,
    api_key: &str,
    secret_key: &str,
) -> Result<String> {
    let url = format!("{ACCESS_TOKEN_URL}?grant_type=client_credentials&client_id={api_key}&client_secret={secret_key}");
    let value: Value = client.get(&url).send().await?.json().await?;
    let result = value["access_token"].as_str().ok_or_else(|| {
        if let Some(err_msg) = value["error_description"].as_str() {
            anyhow!("{err_msg}")
        } else {
            anyhow!("Invalid response data")
        }
    })?;
    Ok(result.to_string())
}
