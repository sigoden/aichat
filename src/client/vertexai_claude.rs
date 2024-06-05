use super::access_token::*;
use super::claude::*;
use super::vertexai::*;
use super::*;

use anyhow::Result;
use async_trait::async_trait;
use reqwest::{Client as ReqwestClient, RequestBuilder};
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize, Default)]
pub struct VertexAIClaudeConfig {
    pub name: Option<String>,
    pub project_id: Option<String>,
    pub location: Option<String>,
    pub adc_file: Option<String>,
    #[serde(default)]
    pub models: Vec<ModelData>,
    pub patches: Option<ModelPatches>,
    pub extra: Option<ExtraConfig>,
}

impl VertexAIClaudeClient {
    config_get_fn!(project_id, get_project_id);
    config_get_fn!(location, get_location);

    pub const PROMPTS: [PromptAction<'static>; 2] = [
        ("project_id", "Project ID", true, PromptKind::String),
        ("location", "Location", true, PromptKind::String),
    ];

    fn chat_completions_builder(
        &self,
        client: &ReqwestClient,
        data: ChatCompletionsData,
    ) -> Result<RequestBuilder> {
        let project_id = self.get_project_id()?;
        let location = self.get_location()?;
        let access_token = get_access_token(self.name())?;

        let base_url = format!("https://{location}-aiplatform.googleapis.com/v1/projects/{project_id}/locations/{location}/publishers");
        let url = format!(
            "{base_url}/anthropic/models/{}:streamRawPredict",
            self.model.name()
        );

        let mut body = claude_build_chat_completions_body(data, &self.model)?;
        self.patch_chat_completions_body(&mut body);
        if let Some(body_obj) = body.as_object_mut() {
            body_obj.remove("model");
        }
        body["anthropic_version"] = "vertex-2023-10-16".into();

        debug!("VertexAIClaude Request: {url} {body}");

        let builder = client.post(url).bearer_auth(access_token).json(&body);

        Ok(builder)
    }
}

#[async_trait]
impl Client for VertexAIClaudeClient {
    client_common_fns!();

    async fn chat_completions_inner(
        &self,
        client: &ReqwestClient,
        data: ChatCompletionsData,
    ) -> Result<ChatCompletionsOutput> {
        prepare_gcloud_access_token(client, self.name(), &self.config.adc_file).await?;
        let builder = self.chat_completions_builder(client, data)?;
        claude_chat_completions(builder).await
    }

    async fn chat_completions_streaming_inner(
        &self,
        client: &ReqwestClient,
        handler: &mut SseHandler,
        data: ChatCompletionsData,
    ) -> Result<()> {
        prepare_gcloud_access_token(client, self.name(), &self.config.adc_file).await?;
        let builder = self.chat_completions_builder(client, data)?;
        claude_chat_completions_streaming(builder, handler).await
    }
}
