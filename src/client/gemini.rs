use super::vertexai::{build_body, send_message, send_message_streaming};
use super::{Client, ExtraConfig, GeminiClient, Model, PromptType, SendData};

use crate::{render::ReplyHandler, utils::PromptKind};

use anyhow::Result;
use async_trait::async_trait;
use reqwest::{Client as ReqwestClient, RequestBuilder};
use serde::Deserialize;

const API_BASE: &str = "https://generativelanguage.googleapis.com/v1beta/models/";

const MODELS: [(&str, usize, &str); 3] = [
    // https://ai.google.dev/models/gemini
    ("gemini-1.0-pro-latest", 30720, "text"),
    ("gemini-1.0-pro-vision-latest", 12288, "text,vision"),
    ("gemini-1.5-pro-latest", 1048576, "text,vision"),
];

#[derive(Debug, Clone, Deserialize, Default)]
pub struct GeminiConfig {
    pub name: Option<String>,
    pub api_key: Option<String>,
    pub block_threshold: Option<String>,
    pub extra: Option<ExtraConfig>,
}

#[async_trait]
impl Client for GeminiClient {
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

impl GeminiClient {
    config_get_fn!(api_key, get_api_key);

    pub const PROMPTS: [PromptType<'static>; 1] =
        [("api_key", "API Key:", true, PromptKind::String)];

    pub fn list_models(local_config: &GeminiConfig) -> Vec<Model> {
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
        let api_key = self.get_api_key()?;

        let func = match data.stream {
            true => "streamGenerateContent",
            false => "generateContent",
        };

        let block_threshold = self.config.block_threshold.clone();

        let body = build_body(data, self.model.name.clone(), block_threshold)?;

        let model = self.model.name.clone();

        let url = format!("{API_BASE}{}:{}?key={}", model, func, api_key);

        debug!("Gemini Request: {url} {body}");

        let builder = client.post(url).json(&body);

        Ok(builder)
    }
}
