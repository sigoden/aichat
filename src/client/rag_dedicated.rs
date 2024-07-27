use super::openai::*;
use super::*;

use anyhow::bail;
use anyhow::Context;
use anyhow::Result;
use reqwest::RequestBuilder;
use serde::Deserialize;
use serde_json::json;
use serde_json::Value;

#[derive(Debug, Clone, Deserialize)]
pub struct RagDedicatedConfig {
    pub name: Option<String>,
    pub api_base: Option<String>,
    pub api_key: Option<String>,
    #[serde(default)]
    pub models: Vec<ModelData>,
    pub patch: Option<RequestPatch>,
    pub extra: Option<ExtraConfig>,
}

impl RagDedicatedClient {
    config_get_fn!(api_base, get_api_base);
    config_get_fn!(api_key, get_api_key);

    pub const PROMPTS: [PromptAction<'static>; 0] = [];

    fn prepare_chat_completions(&self, _data: ChatCompletionsData) -> Result<RequestData> {
        bail!("The client doesn't support chat-completions api");
    }

    fn prepare_embeddings(&self, data: EmbeddingsData) -> Result<RequestData> {
        let api_key = self.get_api_key().ok();
        let api_base = self.get_api_base_ext()?;

        let url = format!("{api_base}/embeddings");

        let body = openai_build_embeddings_body(data, &self.model);

        let mut request_data = RequestData::new(url, body);

        if let Some(api_key) = api_key {
            request_data.bearer_auth(api_key);
        }

        Ok(request_data)
    }

    fn prepare_rerank(&self, data: RerankData) -> Result<RequestData> {
        let api_key = self.get_api_key().ok();
        let api_base = self.get_api_base_ext()?;

        let url = format!("{api_base}/rerank");

        let body = rag_dedicated_build_rerank_body(data, &self.model);

        let mut request_data = RequestData::new(url, body);

        if let Some(api_key) = api_key {
            request_data.bearer_auth(api_key);
        }

        Ok(request_data)
    }

    fn get_api_base_ext(&self) -> Result<String> {
        let api_base = match self.get_api_base() {
            Ok(v) => v,
            Err(err) => {
                match RAG_DEDICATED_PLATFORMS
                    .into_iter()
                    .find_map(|(name, api_base)| {
                        if name == self.model.client_name() {
                            Some(api_base.to_string())
                        } else {
                            None
                        }
                    }) {
                    Some(v) => v,
                    None => return Err(err),
                }
            }
        };
        Ok(api_base)
    }
}

impl_client_trait!(
    RagDedicatedClient,
    no_chat_completions,
    no_chat_completions_streaming,
    openai_embeddings,
    rag_dedicated_rerank
);

pub async fn no_chat_completions(_builder: RequestBuilder) -> Result<ChatCompletionsOutput> {
    bail!("The client doesn't support chat-completions api");
}

pub async fn no_chat_completions_streaming(
    _builder: RequestBuilder,
    _handler: &mut SseHandler,
) -> Result<()> {
    bail!("The client doesn't support chat-completions api")
}

pub async fn rag_dedicated_rerank(builder: RequestBuilder) -> Result<RerankOutput> {
    let res = builder.send().await?;
    let status = res.status();
    let mut data: Value = res.json().await?;
    if !status.is_success() {
        catch_error(&data, status.as_u16())?;
    }
    if data.get("results").is_none() && data.get("data").is_some() {
        if let Some(data_obj) = data.as_object_mut() {
            if let Some(value) = data_obj.remove("data") {
                data_obj.insert("results".to_string(), value);
            }
        }
    }
    let res_body: RagDedicatedRerankResBody =
        serde_json::from_value(data).context("Invalid rerank data")?;
    Ok(res_body.results)
}

#[derive(Deserialize)]
pub struct RagDedicatedRerankResBody {
    pub results: RerankOutput,
}

pub fn rag_dedicated_build_rerank_body(data: RerankData, model: &Model) -> Value {
    let RerankData {
        query,
        documents,
        top_n,
    } = data;

    let mut body = json!({
        "model": model.name(),
        "query": query,
        "documents": documents,
    });
    if model.client_name() == "voyageai" {
        body["top_k"] = top_n.into()
    } else {
        body["top_n"] = top_n.into()
    }
    body
}
