use super::{
    access_token::*, claude::*, vertexai::*, Client, CompletionData, CompletionOutput, ExtraConfig,
    Model, ModelData, ModelPatches, PromptAction, PromptKind, SseHandler, VertexAIClaudeClient,
};

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

    fn request_builder(
        &self,
        client: &ReqwestClient,
        data: CompletionData,
    ) -> Result<RequestBuilder> {
        let project_id = self.get_project_id()?;
        let location = self.get_location()?;
        let access_token = get_access_token(self.name())?;

        let base_url = format!("https://{location}-aiplatform.googleapis.com/v1/projects/{project_id}/locations/{location}/publishers");
        let url = format!(
            "{base_url}/anthropic/models/{}:streamRawPredict",
            self.model.name()
        );

        let mut body = claude_build_body(data, &self.model)?;
        self.patch_request_body(&mut body);
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

    async fn send_message_inner(
        &self,
        client: &ReqwestClient,
        data: CompletionData,
    ) -> Result<CompletionOutput> {
        prepare_gcloud_access_token(client, self.name(), &self.config.adc_file).await?;
        let builder = self.request_builder(client, data)?;
        claude_send_message(builder).await
    }

    async fn send_message_streaming_inner(
        &self,
        client: &ReqwestClient,
        handler: &mut SseHandler,
        data: CompletionData,
    ) -> Result<()> {
        prepare_gcloud_access_token(client, self.name(), &self.config.adc_file).await?;
        let builder = self.request_builder(client, data)?;
        claude_send_message_streaming(builder, handler).await
    }
}
