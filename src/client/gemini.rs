use super::{
    vertexai::*, Client, CompletionData, ExtraConfig, GeminiClient, Model, ModelData, ModelPatches,
    PromptAction, PromptKind,
};

use anyhow::Result;
use reqwest::{Client as ReqwestClient, RequestBuilder};
use serde::Deserialize;

const API_BASE: &str = "https://generativelanguage.googleapis.com/v1beta/models/";

#[derive(Debug, Clone, Deserialize, Default)]
pub struct GeminiConfig {
    pub name: Option<String>,
    pub api_key: Option<String>,
    #[serde(rename = "safetySettings")]
    pub safety_settings: Option<serde_json::Value>,
    #[serde(default)]
    pub models: Vec<ModelData>,
    pub patches: Option<ModelPatches>,
    pub extra: Option<ExtraConfig>,
}

impl GeminiClient {
    config_get_fn!(api_key, get_api_key);

    pub const PROMPTS: [PromptAction<'static>; 1] =
        [("api_key", "API Key:", true, PromptKind::String)];

    fn request_builder(
        &self,
        client: &ReqwestClient,
        data: CompletionData,
    ) -> Result<RequestBuilder> {
        let api_key = self.get_api_key()?;

        let func = match data.stream {
            true => "streamGenerateContent",
            false => "generateContent",
        };

        let mut body = gemini_build_body(data, &self.model)?;
        self.patch_request_body(&mut body);

        let model = &self.model.name();

        let url = format!("{API_BASE}{}:{}?key={}", model, func, api_key);

        debug!("Gemini Request: {url} {body}");

        let builder = client.post(url).json(&body);

        Ok(builder)
    }
}

impl_client_trait!(
    GeminiClient,
    crate::client::vertexai::gemini_send_message,
    crate::client::vertexai::gemini_send_message_streaming
);
