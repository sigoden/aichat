use super::openai::openai_build_body;
use super::{ExtraConfig, MistralClient, Model, ModelConfig, PromptType, SendData};

use crate::utils::PromptKind;

use anyhow::Result;
use async_trait::async_trait;
use reqwest::{Client as ReqwestClient, RequestBuilder};
use serde::Deserialize;

const API_URL: &str = "https://api.mistral.ai/v1/chat/completions";

const MODELS: [(&str, usize, &str); 5] = [
    // https://docs.mistral.ai/platform/endpoints/
    ("open-mistral-7b", 32000, "text"),
    ("open-mixtral-8x7b", 32000, "text"),
    ("mistral-small-latest", 32000, "text"),
    ("mistral-medium-latest", 32000, "text"),
    ("mistral-large-latest", 32000, "text"),
];

#[derive(Debug, Clone, Deserialize)]
pub struct MistralConfig {
    pub name: Option<String>,
    pub api_key: Option<String>,
    #[serde(default)]
    pub models: Vec<ModelConfig>,
    pub extra: Option<ExtraConfig>,
}

openai_compatible_client!(MistralClient);

impl MistralClient {
    list_models_fn!(MistralConfig, &MODELS);
    config_get_fn!(api_key, get_api_key);

    pub const PROMPTS: [PromptType<'static>; 1] =
        [("api_key", "API Key:", false, PromptKind::String)];

    fn request_builder(&self, client: &ReqwestClient, data: SendData) -> Result<RequestBuilder> {
        let api_key = self.get_api_key().ok();

        let body = openai_build_body(data, &self.model);

        let url = API_URL;

        debug!("Mistral Request: {url} {body}");

        let mut builder = client.post(url).json(&body);
        if let Some(api_key) = api_key {
            builder = builder.bearer_auth(api_key);
        }

        Ok(builder)
    }
}
