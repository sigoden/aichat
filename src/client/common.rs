use super::{openai::OpenAIConfig, BuiltinModels, ClientConfig, Message, Model, SseHandler};

use crate::{
    config::{GlobalConfig, Input},
    function::{eval_tool_calls, FunctionDeclaration, ToolCall, ToolCallResult},
    render::{render_error, render_stream},
    utils::{prompt_input_integer, prompt_input_string, tokenize, AbortSignal, PromptKind},
};

use anyhow::{bail, Context, Result};
use async_trait::async_trait;
use fancy_regex::Regex;
use indexmap::IndexMap;
use lazy_static::lazy_static;
use reqwest::{Client as ReqwestClient, ClientBuilder, Proxy, RequestBuilder};
use serde::Deserialize;
use serde_json::{json, Value};
use std::{env, future::Future, time::Duration};
use tokio::{sync::mpsc::unbounded_channel, time::sleep};

const MODELS_YAML: &str = include_str!("../../models.yaml");

lazy_static! {
    pub static ref ALL_CLIENT_MODELS: Vec<BuiltinModels> =
        serde_yaml::from_str(MODELS_YAML).unwrap();
    static ref ESCAPE_SLASH_RE: Regex = Regex::new(r"(?<!\\)/").unwrap();
}

#[macro_export]
macro_rules! register_client {
    (
        $(($module:ident, $name:literal, $config:ident, $client:ident),)+
    ) => {
        $(
            mod $module;
        )+
        $(
            use self::$module::$config;
        )+

        #[derive(Debug, Clone, serde::Deserialize)]
        #[serde(tag = "type")]
        pub enum ClientConfig {
            $(
                #[serde(rename = $name)]
                $config($config),
            )+
            #[serde(other)]
            Unknown,
        }

        #[derive(Debug, Clone, serde::Deserialize)]
        #[serde(tag = "type")]
        pub enum ClientModel {
            $(
                #[serde(rename = $name)]
                $config { models: Vec<ModelData> },
            )+
            #[serde(other)]
            Unknown,
        }


        $(
            #[derive(Debug)]
            pub struct $client {
                global_config: $crate::config::GlobalConfig,
                config: $config,
                model: $crate::client::Model,
            }

            impl $client {
                pub const NAME: &'static str = $name;

                pub fn init(global_config: &$crate::config::GlobalConfig, model: &$crate::client::Model) -> Option<Box<dyn Client>> {
                    let config = global_config.read().clients.iter().find_map(|client_config| {
                        if let ClientConfig::$config(c) = client_config {
                            if Self::name(c) == model.client_name() {
                                return Some(c.clone())
                            }
                        }
                        None
                    })?;

                    Some(Box::new(Self {
                        global_config: global_config.clone(),
                        config,
                        model: model.clone(),
                    }))
                }

                pub fn list_models(local_config: &$config) -> Vec<Model> {
                    let client_name = Self::name(local_config);
                    if local_config.models.is_empty() {
                        if let Some(client_models) = $crate::client::ALL_CLIENT_MODELS.iter().find(|v| {
                            v.platform == $name || ($name == "openai-compatible" && local_config.name.as_deref() == Some(&v.platform))
                        }) {
                            return Model::from_config(client_name, &client_models.models);
                        }
                        vec![]
                    } else {
                        Model::from_config(client_name, &local_config.models)
                    }
                }

                pub fn name(local_config: &$config) -> &str {
                    local_config.name.as_deref().unwrap_or(Self::NAME)
                }
            }

        )+

        pub fn init_client(config: &$crate::config::GlobalConfig, model: Option<$crate::client::Model>) -> anyhow::Result<Box<dyn Client>> {
            let model = model.unwrap_or_else(|| config.read().model.clone());
            None
            $(.or_else(|| $client::init(config, &model)))+
            .ok_or_else(|| {
                anyhow::anyhow!("Unknown client '{}'", model.client_name())
            })
        }

        pub fn list_client_types() -> Vec<&'static str> {
            let mut client_types: Vec<_> = vec![$($client::NAME,)+];
            client_types.extend($crate::client::OPENAI_COMPATIBLE_PLATFORMS.iter().map(|(name, _)| *name));
            client_types
        }

        pub fn create_client_config(client: &str) -> anyhow::Result<(String, serde_json::Value)> {
            $(
                if client == $client::NAME {
                    return create_config(&$client::PROMPTS, $client::NAME)
                }
            )+
            if let Some(ret) = create_openai_compatible_client_config(client)? {
                return Ok(ret);
            }
            anyhow::bail!("Unknown client '{}'", client)
        }

        static mut ALL_CLIENTS: Option<Vec<$crate::client::Model>> = None;

        pub fn list_models(config: &$crate::config::Config) -> Vec<&$crate::client::Model> {
            if unsafe { ALL_CLIENTS.is_none() } {
                let models: Vec<_> = config
                    .clients
                    .iter()
                    .flat_map(|v| match v {
                        $(ClientConfig::$config(c) => $client::list_models(c),)+
                        ClientConfig::Unknown => vec![],
                    })
                    .collect();
                unsafe { ALL_CLIENTS = Some(models) };
            }
            unsafe { ALL_CLIENTS.as_ref().unwrap().iter().collect() }
        }
    };
}

#[macro_export]
macro_rules! client_common_fns {
    () => {
        fn global_config(&self) -> &$crate::config::GlobalConfig {
            &self.global_config
        }

        fn extra_config(&self) -> Option<&$crate::client::ExtraConfig> {
            self.config.extra.as_ref()
        }

        fn patches_config(&self) -> Option<&$crate::client::ModelPatches> {
            self.config.patches.as_ref()
        }

        fn list_models(&self) -> Vec<Model> {
            Self::list_models(&self.config)
        }

        fn name(&self) -> &str {
            Self::name(&self.config)
        }

        fn model(&self) -> &Model {
            &self.model
        }

        fn model_mut(&mut self) -> &mut Model {
            &mut self.model
        }

        fn set_model(&mut self, model: Model) {
            self.model = model;
        }
    };
}

#[macro_export]
macro_rules! impl_client_trait {
    ($client:ident, $send_message:path, $send_message_streaming:path) => {
        #[async_trait::async_trait]
        impl $crate::client::Client for $crate::client::$client {
            client_common_fns!();

            async fn send_message_inner(
                &self,
                client: &reqwest::Client,
                data: $crate::client::CompletionData,
            ) -> anyhow::Result<$crate::client::CompletionOutput> {
                let builder = self.request_builder(client, data)?;
                $send_message(builder).await
            }

            async fn send_message_streaming_inner(
                &self,
                client: &reqwest::Client,
                handler: &mut $crate::client::SseHandler,
                data: $crate::client::CompletionData,
            ) -> Result<()> {
                let builder = self.request_builder(client, data)?;
                $send_message_streaming(builder, handler).await
            }
        }
    };
}

#[macro_export]
macro_rules! config_get_fn {
    ($field_name:ident, $fn_name:ident) => {
        fn $fn_name(&self) -> anyhow::Result<String> {
            let api_key = self.config.$field_name.clone();
            api_key
                .or_else(|| {
                    let env_prefix = Self::name(&self.config);
                    let env_name =
                        format!("{}_{}", env_prefix, stringify!($field_name)).to_ascii_uppercase();
                    std::env::var(&env_name).ok()
                })
                .ok_or_else(|| {
                    anyhow::anyhow!("Miss '{}' in client configuration", stringify!($field_name))
                })
        }
    };
}

#[macro_export]
macro_rules! unsupported_model {
    ($name:expr) => {
        anyhow::bail!("Unsupported model '{}'", $name)
    };
}

#[async_trait]
pub trait Client: Sync + Send {
    fn global_config(&self) -> &GlobalConfig;

    fn extra_config(&self) -> Option<&ExtraConfig>;

    fn patches_config(&self) -> Option<&ModelPatches>;

    #[allow(unused)]
    fn name(&self) -> &str;

    #[allow(unused)]
    fn list_models(&self) -> Vec<Model>;

    fn model(&self) -> &Model;

    fn model_mut(&mut self) -> &mut Model;

    #[allow(unused)]
    fn set_model(&mut self, model: Model);

    fn build_client(&self) -> Result<ReqwestClient> {
        let mut builder = ReqwestClient::builder();
        let extra = self.extra_config();
        let timeout = extra.and_then(|v| v.connect_timeout).unwrap_or(10);
        let proxy = extra.and_then(|v| v.proxy.clone());
        builder = set_proxy(builder, &proxy)?;
        let client = builder
            .connect_timeout(Duration::from_secs(timeout))
            .build()
            .with_context(|| "Failed to build client")?;
        Ok(client)
    }

    async fn send_message(&self, input: Input) -> Result<CompletionOutput> {
        if self.global_config().read().dry_run {
            let content = input.echo_messages();
            return Ok(CompletionOutput::new(&content));
        }
        let client = self.build_client()?;

        let data = input.prepare_completion_data(self.model(), false)?;
        self.send_message_inner(&client, data)
            .await
            .with_context(|| "Failed to get answer")
    }

    async fn send_message_streaming(&self, input: &Input, handler: &mut SseHandler) -> Result<()> {
        async fn watch_abort(abort: AbortSignal) {
            loop {
                if abort.aborted() {
                    break;
                }
                sleep(Duration::from_millis(100)).await;
            }
        }
        let abort = handler.get_abort();
        let input = input.clone();
        tokio::select! {
            ret = async {
                if self.global_config().read().dry_run {
                    let content = input.echo_messages();
                    let tokens = tokenize(&content);
                    for token in tokens {
                        tokio::time::sleep(Duration::from_millis(10)).await;
                        handler.text(token)?;
                    }
                    return Ok(());
                }
                let client = self.build_client()?;
                let data = input.prepare_completion_data(self.model(), true)?;
                self.send_message_streaming_inner(&client, handler, data).await
            } => {
                handler.done()?;
                ret.with_context(|| "Failed to get answer")
            }
            _ = watch_abort(abort.clone()) => {
                handler.done()?;
                Ok(())
            },
        }
    }

    fn patch_request_body(&self, body: &mut Value) {
        let model_name = self.model().name();
        if let Some(patch_data) = select_model_patch(self.patches_config(), model_name) {
            if body.is_object() && patch_data.request_body.is_object() {
                json_patch::merge(body, &patch_data.request_body)
            }
        }
    }

    async fn send_message_inner(
        &self,
        client: &ReqwestClient,
        data: CompletionData,
    ) -> Result<CompletionOutput>;

    async fn send_message_streaming_inner(
        &self,
        client: &ReqwestClient,
        handler: &mut SseHandler,
        data: CompletionData,
    ) -> Result<()>;
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

pub type ModelPatches = IndexMap<String, ModelPatch>;

#[derive(Debug, Clone, Deserialize)]
pub struct ModelPatch {
    #[serde(default)]
    pub request_body: Value,
}

pub fn select_model_patch<'a>(
    patch: Option<&'a ModelPatches>,
    name: &str,
) -> Option<&'a ModelPatch> {
    let patch = patch?;
    for (key, patch_data) in patch {
        let key = ESCAPE_SLASH_RE.replace_all(key, r"\/");
        if let Ok(regex) = Regex::new(&format!("^({key})$")) {
            if let Ok(true) = regex.is_match(name) {
                return Some(patch_data);
            }
        }
    }
    None
}

#[derive(Debug)]
pub struct CompletionData {
    pub messages: Vec<Message>,
    pub temperature: Option<f64>,
    pub top_p: Option<f64>,
    pub functions: Option<Vec<FunctionDeclaration>>,
    pub stream: bool,
}

#[derive(Debug, Clone, Default)]
pub struct CompletionOutput {
    pub text: String,
    pub tool_calls: Vec<ToolCall>,
    pub id: Option<String>,
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
}

impl CompletionOutput {
    pub fn new(text: &str) -> Self {
        Self {
            text: text.to_string(),
            ..Default::default()
        }
    }
}

pub type PromptAction<'a> = (&'a str, &'a str, bool, PromptKind);

pub fn create_config(prompts: &[PromptAction], client: &str) -> Result<(String, Value)> {
    let mut config = json!({
        "type": client,
    });
    let mut model = client.to_string();
    set_client_config_values(prompts, &mut model, &mut config)?;
    let clients = json!(vec![config]);
    Ok((model, clients))
}

pub fn create_openai_compatible_client_config(client: &str) -> Result<Option<(String, Value)>> {
    match super::OPENAI_COMPATIBLE_PLATFORMS
        .iter()
        .find(|(name, _)| client == *name)
    {
        None => Ok(None),
        Some((name, api_base)) => {
            let mut config = json!({
                "type": "openai-compatible",
                "name": name,
                "api_base": api_base,
            });
            let prompts = if ALL_CLIENT_MODELS.iter().any(|v| &v.platform == name) {
                vec![("api_key", "API Key:", false, PromptKind::String)]
            } else {
                vec![
                    ("api_key", "API Key:", false, PromptKind::String),
                    ("models[].name", "Model Name:", true, PromptKind::String),
                    (
                        "models[].max_input_tokens",
                        "Max Input Tokens:",
                        false,
                        PromptKind::Integer,
                    ),
                ]
            };
            let mut model = client.to_string();
            set_client_config_values(&prompts, &mut model, &mut config)?;
            let clients = json!(vec![config]);
            Ok(Some((model, clients)))
        }
    }
}

pub async fn send_stream(
    input: &Input,
    client: &dyn Client,
    config: &GlobalConfig,
    abort: AbortSignal,
) -> Result<(String, Vec<ToolCallResult>)> {
    let (tx, rx) = unbounded_channel();
    let mut handler = SseHandler::new(tx, abort.clone());

    let (send_ret, rend_ret) = tokio::join!(
        client.send_message_streaming(input, &mut handler),
        render_stream(rx, config, abort.clone()),
    );
    if let Err(err) = rend_ret {
        render_error(err, config.read().highlight);
    }
    let (output, calls) = handler.take();
    match send_ret {
        Ok(_) => {
            if !output.is_empty() && !output.ends_with('\n') {
                println!();
            }
            Ok((output, eval_tool_calls(config, calls)?))
        }
        Err(err) => {
            if !output.is_empty() {
                println!();
            }
            Err(err)
        }
    }
}

#[allow(unused)]
pub async fn send_message_as_streaming<F, Fut>(
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
    handler.done()?;

    Ok(())
}

pub fn catch_error(data: &Value, status: u16) -> Result<()> {
    if (200..300).contains(&status) {
        return Ok(());
    }
    debug!("Invalid response, status: {status}, data: {data}");
    if let Some(error) = data["error"].as_object() {
        if let (Some(typ), Some(message)) = (
            get_str_field_from_json_map(error, "type"),
            get_str_field_from_json_map(error, "message"),
        ) {
            bail!("{message} (type: {typ})");
        }
    } else if let Some(error) = data["errors"][0].as_object() {
        if let (Some(code), Some(message)) = (
            get_u64_field_from_json_map(error, "code"),
            get_str_field_from_json_map(error, "message"),
        ) {
            bail!("{message} (status: {code})")
        }
    } else if let Some(error) = data[0]["error"].as_object() {
        if let (Some(status), Some(message)) = (
            get_str_field_from_json_map(error, "status"),
            get_str_field_from_json_map(error, "message"),
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

pub fn get_str_field_from_json_map<'a>(
    map: &'a serde_json::Map<String, Value>,
    field_name: &str,
) -> Option<&'a str> {
    map.get(field_name).and_then(|v| v.as_str())
}

pub fn get_u64_field_from_json_map(
    map: &serde_json::Map<String, Value>,
    field_name: &str,
) -> Option<u64> {
    map.get(field_name).and_then(|v| v.as_u64())
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

fn set_client_config_values(
    list: &[PromptAction],
    model: &mut String,
    client_config: &mut Value,
) -> Result<()> {
    for (path, desc, required, kind) in list {
        match kind {
            PromptKind::String => {
                let value = prompt_input_string(desc, *required)?;
                set_client_config_value(client_config, path, kind, &value);
                if *path == "name" {
                    *model = value;
                }
            }
            PromptKind::Integer => {
                let value = prompt_input_integer(desc, *required)?;
                set_client_config_value(client_config, path, kind, &value);
            }
        }
    }
    Ok(())
}

fn set_client_config_value(client_config: &mut Value, path: &str, kind: &PromptKind, value: &str) {
    let segs: Vec<&str> = path.split('.').collect();
    match segs.as_slice() {
        [name] => client_config[name] = to_json(kind, value),
        [scope, name] => match scope.split_once('[') {
            None => {
                if client_config.get(scope).is_none() {
                    let mut obj = json!({});
                    obj[name] = to_json(kind, value);
                    client_config[scope] = obj;
                } else {
                    client_config[scope][name] = to_json(kind, value);
                }
            }
            Some((scope, _)) => {
                if client_config.get(scope).is_none() {
                    let mut obj = json!({});
                    obj[name] = to_json(kind, value);
                    client_config[scope] = json!([obj]);
                } else {
                    client_config[scope][0][name] = to_json(kind, value);
                }
            }
        },
        _ => {}
    }
}

fn to_json(kind: &PromptKind, value: &str) -> Value {
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

fn set_proxy(builder: ClientBuilder, proxy: &Option<String>) -> Result<ClientBuilder> {
    let proxy = if let Some(proxy) = proxy {
        if proxy.is_empty() || proxy == "-" {
            return Ok(builder);
        }
        proxy.clone()
    } else if let Some(proxy) = ["HTTPS_PROXY", "https_proxy", "ALL_PROXY", "all_proxy"]
        .into_iter()
        .find_map(|v| env::var(v).ok())
    {
        proxy
    } else {
        return Ok(builder);
    };
    let builder =
        builder.proxy(Proxy::all(&proxy).with_context(|| format!("Invalid proxy `{proxy}`"))?);
    Ok(builder)
}
