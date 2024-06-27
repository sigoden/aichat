use super::openai::*;
use super::*;

use anyhow::Result;
use reqwest::{Client as ReqwestClient, RequestBuilder};
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct AzureOpenAIConfig {
    pub name: Option<String>,
    pub api_base: Option<String>,
    pub api_key: Option<String>,
    #[serde(default)]
    pub models: Vec<ModelData>,
    pub patches: Option<ModelPatches>,
    pub extra: Option<ExtraConfig>,
}

impl AzureOpenAIClient {
    config_get_fn!(api_base, get_api_base);
    config_get_fn!(api_key, get_api_key);

    pub const PROMPTS: [PromptAction<'static>; 4] = [
        ("api_base", "API Base:", true, PromptKind::String),
        ("api_key", "API Key:", true, PromptKind::String),
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
        let api_key = self.get_api_key()?;

        let mut body = openai_build_chat_completions_body(data, &self.model);
        self.patch_chat_completions_body(&mut body);

        let url = format!(
            "{}/openai/deployments/{}/chat/completions?api-version=2024-02-01",
            &api_base,
            self.model.name()
        );

        debug!("AzureOpenAI Chat Completions Request: {url} {body}");

        let builder = client.post(url).header("api-key", api_key).json(&body);

        Ok(builder)
    }

    fn embeddings_builder(
        &self,
        client: &ReqwestClient,
        data: EmbeddingsData,
    ) -> Result<RequestBuilder> {
        let api_base = self.get_api_base()?;
        let api_key = self.get_api_key()?;

        let body = openai_build_embeddings_body(data, &self.model);

        let url = format!(
            "{}/openai/deployments/{}/embeddings?api-version=2024-02-01",
            &api_base,
            self.model.name()
        );

        debug!("AzureOpenAI Embeddings Request: {url} {body}");

        let builder = client.post(url).header("api-key", api_key).json(&body);

        Ok(builder)
    }
}

impl_client_trait!(
    AzureOpenAIClient,
    openai_chat_completions,
    openai_chat_completions_streaming,
    openai_embeddings
);
