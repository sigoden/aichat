use super::openai::openai_build_body;
use super::{ExtraConfig, MoonshotClient, Model, PromptType, SendData};

use crate::utils::PromptKind;

use anyhow::Result;
use async_trait::async_trait;
use reqwest::{Client as ReqwestClient, RequestBuilder};
use serde::Deserialize;

const API_URL: &str = "https://api.moonshot.cn/v1/chat/completions";

const MODELS: [(&str, usize, &str); 3] = [
    // https://platform.moonshot.cn/docs/intro
    ("moonshot-v1-8k", 8000, "text"),
    ("moonshot-v1-32k", 32000, "text"),
    ("moonshot-v1-128k", 128000, "text"),
];


#[derive(Debug, Clone, Deserialize)]
pub struct MoonshotConfig {
    pub name: Option<String>,
    pub api_key: Option<String>,
    pub extra: Option<ExtraConfig>,
}

openai_compatible_client!(MoonshotClient);

impl MoonshotClient {
    config_get_fn!(api_key, get_api_key);

    pub const PROMPTS: [PromptType<'static>; 1] = [
        ("api_key", "API Key:", false, PromptKind::String),
    ];

    pub fn list_models(local_config: &MoonshotConfig) -> Vec<Model> {
        let client_name = Self::name(local_config);
        MODELS
            .into_iter()
            .map(|(name, max_input_tokens, capabilities)| {
                Model::new(client_name, name)
                    .set_capabilities(capabilities.into())
                    .set_max_input_tokens(Some(max_input_tokens))
            })
            .collect()
    }

    fn request_builder(&self, client: &ReqwestClient, data: SendData) -> Result<RequestBuilder> {
        let api_key = self.get_api_key().ok();

        let body = openai_build_body(data, self.model.name.clone());

        let url = API_URL;

        debug!("Moonshot Request: {url} {body}");

        let mut builder = client.post(url).json(&body);
        if let Some(api_key) = api_key {
            builder = builder.bearer_auth(api_key);
        }

        Ok(builder)
    }
}
