use super::*;

use crate::{
    config::{GlobalConfig, Input},
    function::{eval_tool_calls, FunctionDeclaration, ToolCall, ToolResult},
    render::render_stream,
    utils::*,
};

use anyhow::{bail, Context, Result};
use fancy_regex::Regex;
use indexmap::IndexMap;
use reqwest::{Client as ReqwestClient, RequestBuilder};
use serde::Deserialize;
use serde_json::{json, Value};
use std::{future::Future, time::Duration};
use tokio::sync::mpsc::unbounded_channel;

const MODELS_YAML: &str = include_str!("../../models.yaml");

lazy_static::lazy_static! {
    pub static ref ALL_MODELS: Vec<BuiltinModels> = serde_yaml::from_str(MODELS_YAML).unwrap();
    static ref ESCAPE_SLASH_RE: Regex = Regex::new(r"(?<!\\)/").unwrap();
}

#[async_trait::async_trait]
pub trait Client: Sync + Send {
    fn global_config(&self) -> &GlobalConfig;

    fn extra_config(&self) -> Option<&ExtraConfig>;

    fn patch_config(&self) -> Option<&RequestPatch>;

    fn name(&self) -> &str;

    fn model(&self) -> &Model;

    fn model_mut(&mut self) -> &mut Model;

    fn build_client(&self) -> Result<ReqwestClient> {
        let mut builder = ReqwestClient::builder();
        let extra = self.extra_config();
        let timeout = extra.and_then(|v| v.connect_timeout).unwrap_or(10);
        let proxy = extra.and_then(|v| v.proxy.clone());
        builder = set_proxy(builder, proxy.as_ref())?;
        let client = builder
            .connect_timeout(Duration::from_secs(timeout))
            .build()
            .with_context(|| "Failed to build client")?;
        Ok(client)
    }

    async fn chat_completions(&self, input: Input) -> Result<ChatCompletionsOutput> {
        if self.global_config().read().dry_run {
            let content = input.echo_messages();
            return Ok(ChatCompletionsOutput::new(&content));
        }
        let client = self.build_client()?;
        let data = input.prepare_completion_data(self.model(), false)?;
        self.chat_completions_inner(&client, data)
            .await
            .with_context(|| "Failed to call chat-completions api")
    }

    async fn chat_completions_streaming(
        &self,
        input: &Input,
        handler: &mut SseHandler,
    ) -> Result<()> {
        let abort_signal = handler.get_abort();
        let input = input.clone();
        tokio::select! {
            ret = async {
                if self.global_config().read().dry_run {
                    let content = input.echo_messages();
                    let tokens = split_content(&content);
                    for token in tokens {
                        tokio::time::sleep(Duration::from_millis(10)).await;
                        handler.text(token)?;
                    }
                    return Ok(());
                }
                let client = self.build_client()?;
                let data = input.prepare_completion_data(self.model(), true)?;
                self.chat_completions_streaming_inner(&client, handler, data).await
            } => {
                handler.done();
                ret.with_context(|| "Failed to call chat-completions api")
            }
            _ = watch_abort_signal(abort_signal) => {
                handler.done();
                Ok(())
            },
        }
    }

    async fn embeddings(&self, data: &EmbeddingsData) -> Result<Vec<Vec<f32>>> {
        let client = self.build_client()?;
        self.embeddings_inner(&client, data)
            .await
            .context("Failed to call embeddings api")
    }

    async fn rerank(&self, data: &RerankData) -> Result<RerankOutput> {
        let client = self.build_client()?;
        self.rerank_inner(&client, data)
            .await
            .context("Failed to call rerank api")
    }

    async fn chat_completions_inner(
        &self,
        client: &ReqwestClient,
        data: ChatCompletionsData,
    ) -> Result<ChatCompletionsOutput>;

    async fn chat_completions_streaming_inner(
        &self,
        client: &ReqwestClient,
        handler: &mut SseHandler,
        data: ChatCompletionsData,
    ) -> Result<()>;

    async fn embeddings_inner(
        &self,
        _client: &ReqwestClient,
        _data: &EmbeddingsData,
    ) -> Result<EmbeddingsOutput> {
        bail!("The client doesn't support embeddings api")
    }

    async fn rerank_inner(
        &self,
        _client: &ReqwestClient,
        _data: &RerankData,
    ) -> Result<RerankOutput> {
        bail!("The client doesn't support rerank api")
    }

    fn request_builder(
        &self,
        client: &reqwest::Client,
        mut request_data: RequestData,
        api_type: ApiType,
    ) -> RequestBuilder {
        self.patch_request_data(&mut request_data, api_type);
        request_data.into_builder(client)
    }

    fn patch_request_data(&self, request_data: &mut RequestData, api_type: ApiType) {
        let map = std::env::var(get_env_name(&format!(
            "patch_{}_{}",
            self.model().client_name(),
            api_type.name(),
        )))
        .ok()
        .and_then(|v| serde_json::from_str(&v).ok())
        .or_else(|| {
            self.patch_config()
                .and_then(|v| api_type.extract_patch(v))
                .cloned()
        });
        let map = match map {
            Some(v) => v,
            _ => return,
        };
        for (key, patch) in map {
            let key = ESCAPE_SLASH_RE.replace_all(&key, r"\/");
            if let Ok(regex) = Regex::new(&format!("^({key})$")) {
                if let Ok(true) = regex.is_match(self.model().name()) {
                    request_data.apply_patch(patch);
                    return;
                }
            }
        }
    }
}

impl Default for ClientConfig {
    fn default() -> Self {
        Self::OpenAIConfig(OpenAIConfig::default())
    }
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct ExtraConfig {
    pub proxy: Option<String>,
    pub connect_timeout: Option<u64>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct RequestPatch {
    pub chat_completions: Option<ApiPatch>,
    pub embeddings: Option<ApiPatch>,
    pub rerank: Option<ApiPatch>,
}

pub type ApiPatch = IndexMap<String, Value>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApiType {
    ChatCompletions,
    Embeddings,
    Rerank,
}

impl ApiType {
    pub fn name(&self) -> &str {
        match self {
            ApiType::ChatCompletions => "chat_completions",
            ApiType::Embeddings => "embeddings",
            ApiType::Rerank => "rerank",
        }
    }
    pub fn extract_patch<'a>(&self, patch: &'a RequestPatch) -> Option<&'a ApiPatch> {
        match self {
            ApiType::ChatCompletions => patch.chat_completions.as_ref(),
            ApiType::Embeddings => patch.embeddings.as_ref(),
            ApiType::Rerank => patch.rerank.as_ref(),
        }
    }
}

pub struct RequestData {
    pub url: String,
    pub headers: IndexMap<String, String>,
    pub body: Value,
}

impl RequestData {
    pub fn new<T>(url: T, body: Value) -> Self
    where
        T: std::fmt::Display,
    {
        Self {
            url: url.to_string(),
            headers: Default::default(),
            body,
        }
    }

    pub fn bearer_auth<T>(&mut self, auth: T)
    where
        T: std::fmt::Display,
    {
        self.headers
            .insert("authorization".into(), format!("Bearer {auth}"));
    }

    pub fn header<K, V>(&mut self, key: K, value: V)
    where
        K: std::fmt::Display,
        V: std::fmt::Display,
    {
        self.headers.insert(key.to_string(), value.to_string());
    }

    pub fn into_builder(self, client: &ReqwestClient) -> RequestBuilder {
        let RequestData { url, headers, body } = self;
        debug!("Request {url} {body}");

        let mut builder = client.post(url);
        for (key, value) in headers {
            builder = builder.header(key, value);
        }
        builder = builder.json(&body);
        builder
    }

    pub fn apply_patch(&mut self, patch: Value) {
        if let Some(patch_url) = patch["url"].as_str() {
            self.url = patch_url.into();
        }
        if let Some(patch_body) = patch.get("body") {
            json_patch::merge(&mut self.body, patch_body)
        }
        if let Some(patch_headers) = patch["headers"].as_object() {
            for (key, value) in patch_headers {
                if let Some(value) = value.as_str() {
                    self.header(key, value)
                }
            }
        }
    }
}

#[derive(Debug)]
pub struct ChatCompletionsData {
    pub messages: Vec<Message>,
    pub temperature: Option<f64>,
    pub top_p: Option<f64>,
    pub functions: Option<Vec<FunctionDeclaration>>,
    pub stream: bool,
}

#[derive(Debug, Clone, Default)]
pub struct ChatCompletionsOutput {
    pub text: String,
    pub tool_calls: Vec<ToolCall>,
    pub id: Option<String>,
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
}

impl ChatCompletionsOutput {
    pub fn new(text: &str) -> Self {
        Self {
            text: text.to_string(),
            ..Default::default()
        }
    }
}

#[derive(Debug)]
pub struct EmbeddingsData {
    pub texts: Vec<String>,
    pub query: bool,
}

impl EmbeddingsData {
    pub fn new(texts: Vec<String>, query: bool) -> Self {
        Self { texts, query }
    }
}

pub type EmbeddingsOutput = Vec<Vec<f32>>;

#[derive(Debug)]
pub struct RerankData {
    pub query: String,
    pub documents: Vec<String>,
    pub top_n: usize,
}

impl RerankData {
    pub fn new(query: String, documents: Vec<String>, top_n: usize) -> Self {
        Self {
            query,
            documents,
            top_n,
        }
    }
}

pub type RerankOutput = Vec<RerankResult>;

#[derive(Debug, Deserialize)]
pub struct RerankResult {
    pub index: usize,
    pub relevance_score: f64,
}

pub type PromptAction<'a> = (&'a str, &'a str, bool, PromptKind);

pub fn create_config(prompts: &[PromptAction], client: &str) -> Result<(String, Value)> {
    let mut config = json!({
        "type": client,
    });
    let mut model = client.to_string();
    set_client_config(prompts, &mut model, &mut config)?;
    let clients = json!(vec![config]);
    Ok((model, clients))
}

pub fn create_openai_compatible_client_config(client: &str) -> Result<Option<(String, Value)>> {
    match super::OPENAI_COMPATIBLE_PLATFORMS
        .into_iter()
        .find(|(name, _)| client == *name)
    {
        None => Ok(None),
        Some((name, api_base)) => {
            let mut config = json!({
                "type": OpenAICompatibleClient::NAME,
                "name": name,
            });
            let mut prompts = vec![];
            if api_base.is_empty() {
                prompts.push(("api_base", "API Base:", true, PromptKind::String));
            } else {
                config["api_base"] = api_base.into();
            }
            prompts.push(("api_key", "API Key:", false, PromptKind::String));
            if !ALL_MODELS.iter().any(|v| v.platform == name) {
                prompts.extend([
                    ("models[].name", "Model Name:", true, PromptKind::String),
                    (
                        "models[].max_input_tokens",
                        "Max Input Tokens:",
                        false,
                        PromptKind::Integer,
                    ),
                ]);
            };
            let mut model = client.to_string();
            set_client_config(&prompts, &mut model, &mut config)?;
            let clients = json!(vec![config]);
            Ok(Some((model, clients)))
        }
    }
}

pub async fn call_chat_completions(
    input: &Input,
    client: &dyn Client,
    config: &GlobalConfig,
) -> Result<(String, Vec<ToolResult>)> {
    let task = client.chat_completions(input.clone());
    let ret = run_with_spinner(task, "Generating").await;
    match ret {
        Ok(ret) => {
            let ChatCompletionsOutput {
                text, tool_calls, ..
            } = ret;
            if !text.is_empty() {
                config.read().print_markdown(&text)?;
            }
            Ok((text, eval_tool_calls(config, tool_calls)?))
        }
        Err(err) => Err(err),
    }
}

pub async fn call_chat_completions_streaming(
    input: &Input,
    client: &dyn Client,
    config: &GlobalConfig,
    abort: AbortSignal,
) -> Result<(String, Vec<ToolResult>)> {
    let (tx, rx) = unbounded_channel();
    let mut handler = SseHandler::new(tx, abort.clone());

    let (send_ret, render_ret) = tokio::join!(
        client.chat_completions_streaming(input, &mut handler),
        render_stream(rx, config, abort.clone()),
    );

    render_ret?;

    let (text, tool_calls) = handler.take();
    match send_ret {
        Ok(_) => {
            if !text.is_empty() && !text.ends_with('\n') {
                println!();
            }
            Ok((text, eval_tool_calls(config, tool_calls)?))
        }
        Err(err) => {
            if !text.is_empty() {
                println!();
            }
            Err(err)
        }
    }
}

#[allow(unused)]
pub async fn chat_completions_as_streaming<F, Fut>(
    builder: RequestBuilder,
    handler: &mut SseHandler,
    f: F,
) -> Result<()>
where
    F: FnOnce(RequestBuilder) -> Fut,
    Fut: Future<Output = Result<String>>,
{
    let text = f(builder).await?;
    handler.text(&text)?;
    handler.done();

    Ok(())
}

pub fn noop_prepare_embeddings<T>(_client: &T, _data: &EmbeddingsData) -> Result<RequestData> {
    bail!("The client doesn't support embeddings api")
}

pub async fn noop_embeddings(_builder: RequestBuilder, _model: &Model) -> Result<EmbeddingsOutput> {
    bail!("The client doesn't support embeddings api")
}

pub fn noop_prepare_rerank<T>(_client: &T, _data: &RerankData) -> Result<RequestData> {
    bail!("The client doesn't support rerank api")
}

pub async fn noop_rerank(_builder: RequestBuilder, _model: &Model) -> Result<RerankOutput> {
    bail!("The client doesn't support rerank api")
}

pub fn catch_error(data: &Value, status: u16) -> Result<()> {
    if (200..300).contains(&status) {
        return Ok(());
    }
    debug!("Invalid response, status: {status}, data: {data}");
    if let Some(error) = data["error"].as_object() {
        if let (Some(typ), Some(message)) = (
            json_str_from_map(error, "type"),
            json_str_from_map(error, "message"),
        ) {
            bail!("{message} (type: {typ})");
        } else if let (Some(typ), Some(message)) = (
            json_str_from_map(error, "code"),
            json_str_from_map(error, "message"),
        ) {
            bail!("{message} (code: {typ})");
        }
    } else if let Some(error) = data["errors"][0].as_object() {
        if let (Some(code), Some(message)) = (
            error.get("code").and_then(|v| v.as_u64()),
            json_str_from_map(error, "message"),
        ) {
            bail!("{message} (status: {code})")
        }
    } else if let Some(error) = data[0]["error"].as_object() {
        if let (Some(status), Some(message)) = (
            json_str_from_map(error, "status"),
            json_str_from_map(error, "message"),
        ) {
            bail!("{message} (status: {status})")
        }
    } else if let (Some(detail), Some(status)) = (data["detail"].as_str(), data["status"].as_i64())
    {
        bail!("{detail} (status: {status})");
    } else if let Some(error) = data["error"].as_str() {
        bail!("{error}");
    } else if let Some(message) = data["message"].as_str() {
        bail!("{message}");
    }
    bail!("Invalid response data: {data} (status: {status})");
}

pub fn json_str_from_map<'a>(
    map: &'a serde_json::Map<String, Value>,
    field_name: &str,
) -> Option<&'a str> {
    map.get(field_name).and_then(|v| v.as_str())
}

pub fn maybe_catch_error(data: &Value) -> Result<()> {
    if let (Some(code), Some(message)) = (data["code"].as_str(), data["message"].as_str()) {
        debug!("Invalid response: {}", data);
        bail!("{message} (code: {code})");
    } else if let (Some(error_code), Some(error_msg)) =
        (data["error_code"].as_number(), data["error_msg"].as_str())
    {
        debug!("Invalid response: {}", data);
        bail!("{error_msg} (error_code: {error_code})");
    }
    Ok(())
}

fn set_client_config(
    list: &[PromptAction],
    model: &mut String,
    client_config: &mut Value,
) -> Result<()> {
    let env_prefix = model.clone();
    for (path, desc, required, kind) in list {
        let mut required = *required;
        if required {
            let env_name = format!("{env_prefix}_{path}").to_ascii_uppercase();
            if std::env::var(&env_name).is_ok() {
                required = false;
            }
        }
        match kind {
            PromptKind::String => {
                let value = prompt_input_string(desc, required)?;
                set_client_config_value(client_config, path, kind, &value);
                if *path == "name" {
                    *model = value;
                }
            }
            PromptKind::Integer => {
                let value = prompt_input_integer(desc, required)?;
                set_client_config_value(client_config, path, kind, &value);
            }
        }
    }
    Ok(())
}

fn set_client_config_value(client_config: &mut Value, path: &str, kind: &PromptKind, value: &str) {
    let segs: Vec<&str> = path.split('.').collect();
    match segs.as_slice() {
        [name] => client_config[name] = prompt_value_to_json(kind, value),
        [scope, name] => match scope.split_once('[') {
            None => {
                if client_config.get(scope).is_none() {
                    let mut obj = json!({});
                    obj[name] = prompt_value_to_json(kind, value);
                    client_config[scope] = obj;
                } else {
                    client_config[scope][name] = prompt_value_to_json(kind, value);
                }
            }
            Some((scope, _)) => {
                if client_config.get(scope).is_none() {
                    let mut obj = json!({});
                    obj[name] = prompt_value_to_json(kind, value);
                    client_config[scope] = json!([obj]);
                } else {
                    client_config[scope][0][name] = prompt_value_to_json(kind, value);
                }
            }
        },
        _ => {}
    }
}

fn prompt_value_to_json(kind: &PromptKind, value: &str) -> Value {
    if value.is_empty() {
        return Value::Null;
    }
    match kind {
        PromptKind::String => value.into(),
        PromptKind::Integer => match value.parse::<i32>() {
            Ok(value) => value.into(),
            Err(_) => value.into(),
        },
    }
}

fn split_content(text: &str) -> Vec<&str> {
    if text.is_ascii() {
        text.split_inclusive(|c: char| c.is_ascii_whitespace())
            .collect()
    } else {
        unicode_segmentation::UnicodeSegmentation::graphemes(text, true).collect()
    }
}
