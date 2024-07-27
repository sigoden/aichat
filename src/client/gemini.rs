use super::vertexai::*;
use super::*;

use anyhow::{Context, Result};
use reqwest::RequestBuilder;
use serde::Deserialize;
use serde_json::{json, Value};

const API_BASE: &str = "https://generativelanguage.googleapis.com/v1beta/models/";

#[derive(Debug, Clone, Deserialize, Default)]
pub struct GeminiConfig {
    pub name: Option<String>,
    pub api_key: Option<String>,
    #[serde(default)]
    pub models: Vec<ModelData>,
    pub patch: Option<RequestPatch>,
    pub extra: Option<ExtraConfig>,
}

impl GeminiClient {
    config_get_fn!(api_key, get_api_key);

    pub const PROMPTS: [PromptAction<'static>; 1] =
        [("api_key", "API Key:", true, PromptKind::String)];

    fn prepare_chat_completions(&self, data: ChatCompletionsData) -> Result<RequestData> {
        let api_key = self.get_api_key()?;

        let func = match data.stream {
            true => "streamGenerateContent",
            false => "generateContent",
        };

        let url = format!("{API_BASE}{}:{}?key={}", &self.model.name(), func, api_key);

        let body = gemini_build_chat_completions_body(data, &self.model)?;

        let request_data = RequestData::new(url, body);

        Ok(request_data)
    }

    fn prepare_embeddings(&self, data: EmbeddingsData) -> Result<RequestData> {
        let api_key = self.get_api_key()?;

        let url = format!(
            "{API_BASE}{}:embedContent?key={}",
            &self.model.name(),
            api_key
        );

        let body = json!({
            "content": {
                "parts": [
                    {
                        "text": data.texts[0],
                    }
                ]
            }
        });

        let request_data = RequestData::new(url, body);

        Ok(request_data)
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
