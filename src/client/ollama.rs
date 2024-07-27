use super::*;

use anyhow::{bail, Context, Result};
use reqwest::{Client as ReqwestClient, RequestBuilder};
use serde::Deserialize;
use serde_json::{json, Value};

#[derive(Debug, Clone, Deserialize, Default)]
pub struct OllamaConfig {
    pub name: Option<String>,
    pub api_base: Option<String>,
    pub api_auth: Option<String>,
    #[serde(default)]
    pub models: Vec<ModelData>,
    pub patch: Option<ModelPatch>,
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

    fn chat_completions_builder(
        &self,
        client: &ReqwestClient,
        data: ChatCompletionsData,
    ) -> Result<RequestBuilder> {
        let api_base = self.get_api_base()?;
        let api_auth = self.get_api_auth().ok();

        let mut body = build_chat_completions_body(data, &self.model)?;
        self.patch_chat_completions_body(&mut body);

        let url = format!("{api_base}/api/chat");

        debug!("Ollama Chat Completions Request: {url} {body}");

        let mut builder = client.post(url).json(&body);
        if let Some(api_auth) = api_auth {
            builder = builder.header("Authorization", api_auth)
        }

        Ok(builder)
    }

    fn embeddings_builder(
        &self,
        client: &ReqwestClient,
        data: EmbeddingsData,
    ) -> Result<RequestBuilder> {
        let api_base = self.get_api_base()?;
        let api_auth = self.get_api_auth().ok();

        let body = json!({
            "model": self.model.name(),
            "input": data.texts,
        });

        let url = format!("{api_base}/api/embed");

        debug!("Ollama Embeddings Request: {url} {body}");

        let mut builder = client.post(url).json(&body);
        if let Some(api_auth) = api_auth {
            builder = builder.header("Authorization", api_auth)
        }

        Ok(builder)
    }
}

impl_client_trait!(
    OllamaClient,
    chat_completions,
    chat_completions_streaming,
    embeddings
);

async fn chat_completions(builder: RequestBuilder) -> Result<ChatCompletionsOutput> {
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

async fn embeddings(builder: RequestBuilder) -> Result<EmbeddingsOutput> {
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
