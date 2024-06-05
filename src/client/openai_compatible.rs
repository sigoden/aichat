use super::openai::*;
use super::*;

use anyhow::Result;
use reqwest::{Client as ReqwestClient, RequestBuilder};
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct OpenAICompatibleConfig {
    pub name: Option<String>,
    pub api_base: Option<String>,
    pub api_key: Option<String>,
    pub chat_endpoint: Option<String>,
    #[serde(default)]
    pub models: Vec<ModelData>,
    pub patches: Option<ModelPatches>,
    pub extra: Option<ExtraConfig>,
}

impl OpenAICompatibleClient {
    config_get_fn!(api_base, get_api_base);
    config_get_fn!(api_key, get_api_key);

    pub const PROMPTS: [PromptAction<'static>; 5] = [
        ("name", "Platform Name:", true, PromptKind::String),
        ("api_base", "API Base:", true, PromptKind::String),
        ("api_key", "API Key:", false, PromptKind::String),
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
        let api_key = self.get_api_key().ok();
        let api_base = self.get_api_base_ext()?;

        let mut body = openai_build_chat_completions_body(data, &self.model);
        self.patch_chat_completions_body(&mut body);

        let chat_endpoint = self
            .config
            .chat_endpoint
            .as_deref()
            .unwrap_or("/chat/completions");

        let url = format!("{api_base}{chat_endpoint}");

        debug!("OpenAICompatible Chat Completions Request: {url} {body}");

        let mut builder = client.post(url).json(&body);
        if let Some(api_key) = api_key {
            builder = builder.bearer_auth(api_key);
        }

        Ok(builder)
    }

    fn embeddings_builder(
        &self,
        client: &ReqwestClient,
        data: EmbeddingsData,
    ) -> Result<RequestBuilder> {
        let api_key = self.get_api_key()?;
        let api_base = self.get_api_base_ext()?;

        let body = openai_build_embeddings_body(data, &self.model);

        let url = format!("{api_base}/embeddings");

        debug!("OpenAICompatible Embeddings Request: {url} {body}");

        let builder = client.post(url).bearer_auth(api_key).json(&body);

        Ok(builder)
    }

    fn get_api_base_ext(&self) -> Result<String> {
        let api_base = match self.get_api_base() {
            Ok(v) => v,
            Err(err) => {
                match OPENAI_COMPATIBLE_PLATFORMS
                    .into_iter()
                    .find_map(|(name, api_base)| {
                        if name == self.model.client_name() {
                            Some(api_base.to_string())
                        } else {
                            None
                        }
                    }) {
                    Some(v) => v,
                    None => return Err(err),
                }
            }
        };
        Ok(api_base)
    }
}

impl_client_trait!(
    OpenAICompatibleClient,
    openai_chat_completions,
    openai_chat_completions_streaming,
    openai_embeddings
);
