use super::vertexai::{gemini_build_body, gemini_send_message, gemini_send_message_streaming};
use super::{
    Client, ExtraConfig, GeminiClient, Model, ModelConfig, PromptType, ReplyHandler, SendData,
};

use crate::utils::PromptKind;

use anyhow::Result;
use async_trait::async_trait;
use reqwest::{Client as ReqwestClient, RequestBuilder};
use serde::Deserialize;

const API_BASE: &str = "https://generativelanguage.googleapis.com/v1beta/models/";

#[derive(Debug, Clone, Deserialize, Default)]
pub struct GeminiConfig {
    pub name: Option<String>,
    pub api_key: Option<String>,
    pub block_threshold: Option<String>,
    #[serde(default)]
    pub models: Vec<ModelConfig>,
    pub extra: Option<ExtraConfig>,
}

#[async_trait]
impl Client for GeminiClient {
    client_common_fns!();

    async fn send_message_inner(&self, client: &ReqwestClient, data: SendData) -> Result<String> {
        let builder = self.request_builder(client, data)?;
        gemini_send_message(builder).await
    }

    async fn send_message_streaming_inner(
        &self,
        client: &ReqwestClient,
        handler: &mut ReplyHandler,
        data: SendData,
    ) -> Result<()> {
        let builder = self.request_builder(client, data)?;
        gemini_send_message_streaming(builder, handler).await
    }
}

impl GeminiClient {
    list_models_fn!(
        GeminiConfig,
        [
            // https://ai.google.dev/models/gemini
            ("gemini-1.0-pro-latest", "text", 30720),
            ("gemini-1.0-pro-vision-latest", "text,vision", 12288),
            ("gemini-1.5-pro-latest", "text,vision", 1048576),
        ]
    );
    config_get_fn!(api_key, get_api_key);

    pub const PROMPTS: [PromptType<'static>; 1] =
        [("api_key", "API Key:", true, PromptKind::String)];

    fn request_builder(&self, client: &ReqwestClient, data: SendData) -> Result<RequestBuilder> {
        let api_key = self.get_api_key()?;

        let func = match data.stream {
            true => "streamGenerateContent",
            false => "generateContent",
        };

        let block_threshold = self.config.block_threshold.clone();

        let body = gemini_build_body(data, &self.model, block_threshold)?;

        let model = &self.model.name;

        let url = format!("{API_BASE}{}:{}?key={}", model, func, api_key);

        debug!("Gemini Request: {url} {body}");

        let builder = client.post(url).json(&body);

        Ok(builder)
    }
}
