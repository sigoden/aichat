use super::openai::*;
use super::*;

use anyhow::Result;
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct AzureOpenAIConfig {
    pub name: Option<String>,
    pub api_base: Option<String>,
    pub api_key: Option<String>,
    #[serde(default)]
    pub models: Vec<ModelData>,
    pub patch: Option<RequestPatch>,
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

    fn prepare_chat_completions(&self, data: ChatCompletionsData) -> Result<RequestData> {
        let api_base = self.get_api_base()?;
        let api_key = self.get_api_key()?;

        let url = format!(
            "{}/openai/deployments/{}/chat/completions?api-version=2024-02-01",
            &api_base,
            self.model.name()
        );

        let body = openai_build_chat_completions_body(data, &self.model);

        let mut request_data = RequestData::new(url, body);

        request_data.header("api-key", api_key);

        Ok(request_data)
    }

    fn prepare_embeddings(&self, data: EmbeddingsData) -> Result<RequestData> {
        let api_base = self.get_api_base()?;
        let api_key = self.get_api_key()?;

        let url = format!(
            "{}/openai/deployments/{}/embeddings?api-version=2024-02-01",
            &api_base,
            self.model.name()
        );

        let body = openai_build_embeddings_body(data, &self.model);

        let mut request_data = RequestData::new(url, body);

        request_data.header("api-key", api_key);

        Ok(request_data)
    }
}

impl_client_trait!(
    AzureOpenAIClient,
    openai_chat_completions,
    openai_chat_completions_streaming,
    openai_embeddings
);
