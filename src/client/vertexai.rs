use super::{
    Client, ExtraConfig, VertexAIClient, Model, PromptType,
    SendData, TokensCountFactors,
};
use super::gemini::{build_body, send_message, send_message_streaming};

use crate::{render::ReplyHandler, utils::PromptKind};

use anyhow::Result;
use async_trait::async_trait;
use reqwest::{Client as ReqwestClient, RequestBuilder};
use serde::Deserialize;

const MODELS: [(&str, usize, &str); 2] = [
    ("gemini-pro", 32760, "text"),
    ("gemini-pro-vision", 16384, "text,vision"),
];

const TOKENS_COUNT_FACTORS: TokensCountFactors = (5, 2);

#[derive(Debug, Clone, Deserialize, Default)]
pub struct VertexAIConfig {
    pub name: Option<String>,
    pub api_base: Option<String>,
    pub api_key: Option<String>,
    pub extra: Option<ExtraConfig>,
}

#[async_trait]
impl Client for VertexAIClient {
    client_common_fns!();

    async fn send_message_inner(&self, client: &ReqwestClient, data: SendData) -> Result<String> {
        let builder = self.request_builder(client, data)?;
        send_message(builder).await
    }

    async fn send_message_streaming_inner(
        &self,
        client: &ReqwestClient,
        handler: &mut ReplyHandler,
        data: SendData,
    ) -> Result<()> {
        let builder = self.request_builder(client, data)?;
        send_message_streaming(builder, handler).await
    }
}

impl VertexAIClient {
    config_get_fn!(api_base, get_api_base);
    config_get_fn!(api_key, get_api_key);

    pub const PROMPTS: [PromptType<'static>; 2] = [
        ("api_base", "API Base:", true, PromptKind::String),
        ("api_key", "API Key:", true, PromptKind::String),
    ];

    pub fn list_models(local_config: &VertexAIConfig) -> Vec<Model> {
        let client_name = Self::name(local_config);
        MODELS
            .into_iter()
            .map(|(name, max_tokens, capabilities)| {
                Model::new(client_name, name)
                    .set_capabilities(capabilities.into())
                    .set_max_tokens(Some(max_tokens))
                    .set_tokens_count_factors(TOKENS_COUNT_FACTORS)
            })
            .collect()
    }

    fn request_builder(&self, client: &ReqwestClient, data: SendData) -> Result<RequestBuilder> {
        let api_base = self.get_api_base()?;
        let api_key = self.get_api_key()?;

        let func = match data.stream {
            true => "streamGenerateContent",
            false => "generateContent",
        };

        let body = build_body(data, self.model.name.clone())?;

        let model = self.model.name.clone();

        let url = format!("{api_base}/{}:{}", model, func);

        debug!("VertexAI Request: {url} {body}");

        let builder = client.post(url).bearer_auth(api_key).json(&body);

        Ok(builder)
    }
}
