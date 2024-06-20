use super::*;

use anyhow::{anyhow, bail, Context, Result};
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
    pub patches: Option<ModelPatches>,
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
            "prompt": data.texts[0],
        });

        let url = format!("{api_base}/api/embeddings");

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
    let text = data["message"]["content"]
        .as_str()
        .ok_or_else(|| anyhow!("Invalid response data: {data}"))?;
    Ok(ChatCompletionsOutput::new(text))
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
    let output = vec![res_body.embedding];
    Ok(output)
}

#[derive(Deserialize)]
struct EmbeddingsResBody {
    embedding: Vec<f32>,
}

fn build_chat_completions_body(data: ChatCompletionsData, model: &Model) -> Result<Value> {
    let ChatCompletionsData {
        messages,
        temperature,
        top_p,
        functions: _,
        stream,
    } = data;

    let mut is_tool_call = false;
    let mut network_image_urls = vec![];

    let messages: Vec<Value> = messages
        .into_iter()
        .map(|message| {
            let role = message.role;
            match message.content {
                MessageContent::Text(text) => json!({
                    "role": role,
                    "content": text,
                }),
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
                    json!({ "role": role, "content": content, "images": images })
                }
                MessageContent::ToolResults(_) => {
                    is_tool_call = true;
                    json!({ "role": role })
                }
            }
        })
        .collect();

    if is_tool_call {
        bail!("The client does not support function calling",);
    }

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

    Ok(body)
}
