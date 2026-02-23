use super::openai::*;
use super::*;

use anyhow::Result;
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct AzureOpenAIConfig {
    pub name: Option<String>,
    pub api_base: Option<String>,
    pub api_key: Option<String>,
    pub api_key_helper: Option<String>,
    #[serde(default)]
    pub models: Vec<ModelData>,
    pub patch: Option<RequestPatch>,
    pub extra: Option<ExtraConfig>,
}

impl AzureOpenAIClient {
    config_get_fn!(api_base, get_api_base);
    config_get_fn_with_helper!(api_key, get_api_key);

    pub const PROMPTS: [PromptAction<'static>; 3] = [
        PromptAction {
            key: "api_base",
            description: "API Base",
            help_message: Some("e.g. https://{RESOURCE}.openai.azure.com"),
            required: true,
            default: None,
        },
        PromptAction {
            key: "api_key",
            description: "API Key",
            help_message: Some("Leave empty if you want to use api_key_helper instead"),
            required: false,
            default: None,
        },
        PromptAction {
            key: "api_key_helper",
            description: "API Key Helper",
            help_message: Some("Leave empty if you have provided api_key"),
            required: false,
            default: None,
        },
    ];
}

impl_client_trait!(
    AzureOpenAIClient,
    (
        prepare_chat_completions,
        openai_chat_completions,
        openai_chat_completions_streaming
    ),
    (prepare_embeddings, openai_embeddings),
    (noop_prepare_rerank, noop_rerank),
);

fn prepare_chat_completions(
    self_: &AzureOpenAIClient,
    data: ChatCompletionsData,
) -> Result<RequestData> {
    let api_base = self_.get_api_base()?;
    let api_key = self_.get_api_key()?;

    let url = format!(
        "{}/openai/deployments/{}/chat/completions?api-version=2024-12-01-preview",
        &api_base,
        self_.model.real_name()
    );

    let body = openai_build_chat_completions_body(data, &self_.model);

    let mut request_data = RequestData::new(url, body);

    request_data.header("api-key", api_key);

    Ok(request_data)
}

fn prepare_embeddings(self_: &AzureOpenAIClient, data: &EmbeddingsData) -> Result<RequestData> {
    let api_base = self_.get_api_base()?;
    let api_key = self_.get_api_key()?;

    let url = format!(
        "{}/openai/deployments/{}/embeddings?api-version=2024-10-21",
        &api_base,
        self_.model.real_name()
    );

    let body = openai_build_embeddings_body(data, &self_.model);

    let mut request_data = RequestData::new(url, body);

    request_data.header("api-key", api_key);

    Ok(request_data)
}
