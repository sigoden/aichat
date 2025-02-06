use super::openai::*;
use super::*;

use anyhow::{Context, Result};
use reqwest::RequestBuilder;
use serde::Deserialize;
use serde_json::{json, Value};

#[derive(Debug, Clone, Deserialize)]
pub struct OpenAICompatibleConfig {
    pub name: Option<String>,
    pub api_base: Option<String>,
    pub api_key: Option<String>,
    #[serde(default)]
    pub models: Vec<ModelData>,
    pub patch: Option<RequestPatch>,
    pub extra: Option<ExtraConfig>,
}

impl OpenAICompatibleClient {
    config_get_fn!(api_base, get_api_base);
    config_get_fn!(api_key, get_api_key);

    pub const PROMPTS: [PromptAction<'static>; 0] = [];
}

impl_client_trait!(
    OpenAICompatibleClient,
    (
        prepare_chat_completions,
        openai_chat_completions,
        openai_chat_completions_streaming
    ),
    (prepare_embeddings, openai_embeddings),
    (prepare_rerank, generic_rerank),
);

fn prepare_chat_completions(
    self_: &OpenAICompatibleClient,
    data: ChatCompletionsData,
) -> Result<RequestData> {
    let api_key = self_.get_api_key().ok();
    let api_base = get_api_base_ext(self_)?;

    let url = format!("{api_base}/chat/completions");

    let body = openai_build_chat_completions_body(data, &self_.model);

    let mut request_data = RequestData::new(url, body);

    if let Some(api_key) = api_key {
        request_data.bearer_auth(api_key);
    }

    Ok(request_data)
}

fn prepare_embeddings(
    self_: &OpenAICompatibleClient,
    data: &EmbeddingsData,
) -> Result<RequestData> {
    let api_key = self_.get_api_key().ok();
    let api_base = get_api_base_ext(self_)?;

    let url = format!("{api_base}/embeddings");

    let body = openai_build_embeddings_body(data, &self_.model);

    let mut request_data = RequestData::new(url, body);

    if let Some(api_key) = api_key {
        request_data.bearer_auth(api_key);
    }

    Ok(request_data)
}

fn prepare_rerank(self_: &OpenAICompatibleClient, data: &RerankData) -> Result<RequestData> {
    let api_key = self_.get_api_key().ok();
    let api_base = get_api_base_ext(self_)?;

    let url = if self_.name().starts_with("ernie") {
        format!("{api_base}/rerankers")
    } else {
        format!("{api_base}/rerank")
    };

    let body = generic_build_rerank_body(data, &self_.model);

    let mut request_data = RequestData::new(url, body);

    if let Some(api_key) = api_key {
        request_data.bearer_auth(api_key);
    }

    Ok(request_data)
}

fn get_api_base_ext(self_: &OpenAICompatibleClient) -> Result<String> {
    let api_base = match self_.get_api_base() {
        Ok(v) => v,
        Err(err) => {
            match OPENAI_COMPATIBLE_PROVIDERS
                .into_iter()
                .find_map(|(name, api_base)| {
                    if name == self_.model.client_name() {
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
    Ok(api_base.trim_end_matches('/').to_string())
}

pub async fn generic_rerank(builder: RequestBuilder, _model: &Model) -> Result<RerankOutput> {
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
    let res_body: GenericRerankResBody =
        serde_json::from_value(data).context("Invalid rerank data")?;
    Ok(res_body.results)
}

#[derive(Deserialize)]
pub struct GenericRerankResBody {
    pub results: RerankOutput,
}

pub fn generic_build_rerank_body(data: &RerankData, model: &Model) -> Value {
    let RerankData {
        query,
        documents,
        top_n,
    } = data;

    let mut body = json!({
        "model": model.real_name(),
        "query": query,
        "documents": documents,
    });
    if model.client_name().starts_with("voyageai") {
        body["top_k"] = (*top_n).into()
    } else {
        body["top_n"] = (*top_n).into()
    }
    body
}
