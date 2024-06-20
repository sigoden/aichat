use super::vertexai::*;
use super::*;

use anyhow::{Context, Result};
use reqwest::{Client as ReqwestClient, RequestBuilder};
use serde::Deserialize;
use serde_json::{json, Value};

const API_BASE: &str = "https://generativelanguage.googleapis.com/v1beta/models/";

#[derive(Debug, Clone, Deserialize, Default)]
pub struct GeminiConfig {
    pub name: Option<String>,
    pub api_key: Option<String>,
    #[serde(default)]
    pub models: Vec<ModelData>,
    pub patches: Option<ModelPatches>,
    pub extra: Option<ExtraConfig>,
}

impl GeminiClient {
    config_get_fn!(api_key, get_api_key);

    pub const PROMPTS: [PromptAction<'static>; 1] =
        [("api_key", "API Key:", true, PromptKind::String)];

    fn chat_completions_builder(
        &self,
        client: &ReqwestClient,
        data: ChatCompletionsData,
    ) -> Result<RequestBuilder> {
        let api_key = self.get_api_key()?;

        let func = match data.stream {
            true => "streamGenerateContent",
            false => "generateContent",
        };

        let mut body = gemini_build_chat_completions_body(data, &self.model)?;
        self.patch_chat_completions_body(&mut body);

        let url = format!("{API_BASE}{}:{}?key={}", &self.model.name(), func, api_key);

        debug!("Gemini Chat Completions Request: {url} {body}");

        let builder = client.post(url).json(&body);

        Ok(builder)
    }

    fn embeddings_builder(
        &self,
        client: &ReqwestClient,
        data: EmbeddingsData,
    ) -> Result<RequestBuilder> {
        let api_key = self.get_api_key()?;

        let body = json!({
            "content": {
                "parts": [
                    {
                        "text": data.texts[0],
                    }
                ]
            }
        });

        let url = format!(
            "{API_BASE}{}:embedContent?key={}",
            &self.model.name(),
            api_key
        );

        debug!("Gemini Embeddings Request: {url} {body}");

        let builder = client.post(url).json(&body);

        Ok(builder)
    }
}

impl_client_trait!(
    GeminiClient,
    gemini_chat_completions,
    gemini_chat_completions_streaming,
    gemini_embeddings
);

async fn gemini_embeddings(builder: RequestBuilder) -> Result<EmbeddingsOutput> {
    let res = builder.send().await?;
    let status = res.status();
    let data: Value = res.json().await?;
    if !status.is_success() {
        catch_error(&data, status.as_u16())?;
    }
    let res_body: EmbeddingsResBody =
        serde_json::from_value(data).context("Invalid embeddings data")?;
    let output = vec![res_body.embedding.values];
    Ok(output)
}

#[derive(Deserialize)]
struct EmbeddingsResBody {
    embedding: EmbeddingsResBodyEmbedding,
}

#[derive(Deserialize)]
struct EmbeddingsResBodyEmbedding {
    values: Vec<f32>,
}
