use super::{
    openai::*, AzureOpenAIClient, ChatCompletionsData, Client, ExtraConfig, Model, ModelData,
    ModelPatches, PromptAction, PromptKind,
};

use anyhow::Result;
use reqwest::{Client as ReqwestClient, RequestBuilder};
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct AzureOpenAIConfig {
    pub name: Option<String>,
    pub api_base: Option<String>,
    pub api_key: Option<String>,
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

    fn request_builder(
        &self,
        client: &ReqwestClient,
        data: ChatCompletionsData,
    ) -> Result<RequestBuilder> {
        let api_base = self.get_api_base()?;
        let api_key = self.get_api_key()?;

        let mut body = openai_build_body(data, &self.model);
        self.patch_request_body(&mut body);

        let url = format!(
            "{}/openai/deployments/{}/chat/completions?api-version=2024-02-01",
            &api_base,
            self.model.name()
        );

        debug!("AzureOpenAI Request: {url} {body}");

        let builder = client.post(url).header("api-key", api_key).json(&body);

        Ok(builder)
    }
}

impl_client_trait!(
    AzureOpenAIClient,
    crate::client::openai::openai_chat_completions,
    crate::client::openai::openai_chat_completions_streaming
);
