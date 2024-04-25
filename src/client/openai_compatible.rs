use super::openai::openai_build_body;
use super::{ExtraConfig, Model, ModelConfig, OpenAICompatibleClient, PromptType, SendData};

use crate::utils::PromptKind;

use anyhow::Result;
use reqwest::{Client as ReqwestClient, RequestBuilder};
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct OpenAICompatibleConfig {
    pub name: Option<String>,
    pub api_base: String,
    pub api_key: Option<String>,
    pub chat_endpoint: Option<String>,
    pub models: Vec<ModelConfig>,
    pub extra: Option<ExtraConfig>,
}

impl OpenAICompatibleClient {
    list_models_fn!(OpenAICompatibleConfig);
    config_get_fn!(api_key, get_api_key);

    pub const PROMPTS: [PromptType<'static>; 5] = [
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

    fn request_builder(&self, client: &ReqwestClient, data: SendData) -> Result<RequestBuilder> {
        let api_key = self.get_api_key().ok();

        let mut body = openai_build_body(data, &self.model);
        self.model.merge_extra_fields(&mut body);

        let chat_endpoint = self
            .config
            .chat_endpoint
            .as_deref()
            .unwrap_or("/chat/completions");

        let url = format!("{}{chat_endpoint}", self.config.api_base);

        debug!("OpenAICompatible Request: {url} {body}");

        let mut builder = client.post(url).json(&body);
        if let Some(api_key) = api_key {
            builder = builder.bearer_auth(api_key);
        }

        Ok(builder)
    }
}

impl_client_trait!(
    OpenAICompatibleClient,
    crate::client::openai::openai_send_message,
    crate::client::openai::openai_send_message_streaming
);
