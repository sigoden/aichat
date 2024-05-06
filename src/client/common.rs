use super::{openai::OpenAIConfig, BuiltinModels, ClientConfig, Message, Model, SseHandler};

use crate::{
    config::{GlobalConfig, Input},
    render::{render_error, render_stream},
    utils::{prompt_input_integer, prompt_input_string, tokenize, AbortSignal, PromptKind},
};

use anyhow::{bail, Context, Result};
use async_trait::async_trait;
use futures_util::{Stream, StreamExt};
use lazy_static::lazy_static;
use reqwest::{Client as ReqwestClient, ClientBuilder, Proxy, RequestBuilder};
use reqwest_eventsource::{Error as EventSourceError, Event, RequestBuilderExt};
use serde::Deserialize;
use serde_json::{json, Value};
use std::{env, future::Future, time::Duration};
use tokio::{sync::mpsc::unbounded_channel, time::sleep};

const MODELS_YAML: &str = include_str!("../../models.yaml");

lazy_static! {
    pub static ref ALL_CLIENT_MODELS: Vec<BuiltinModels> =
        serde_yaml::from_str(MODELS_YAML).unwrap();
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
                $config { models: Vec<ModelConfig> },
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

                pub fn init(global_config: &$crate::config::GlobalConfig) -> Option<Box<dyn Client>> {
                    let model = global_config.read().model.clone();
                    let config = global_config.read().clients.iter().find_map(|client_config| {
                        if let ClientConfig::$config(c) = client_config {
                            if Self::name(c) == &model.client_name {
                                return Some(c.clone())
                            }
                        }
                        None
                    })?;

                    Some(Box::new(Self {
                        global_config: global_config.clone(),
                        config,
                        model,
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

        pub fn init_client(config: &$crate::config::GlobalConfig) -> anyhow::Result<Box<dyn Client>> {
            None
            $(.or_else(|| $client::init(config)))+
            .ok_or_else(|| {
                anyhow::anyhow!("Unknown client '{}'", &config.read().model.client_name)
            })
        }

        pub fn ensure_model_capabilities(client: &mut dyn Client, capabilities: $crate::client::ModelCapabilities) -> anyhow::Result<()> {
            if !client.model().capabilities.contains(capabilities) {
                let models = client.list_models();
                if let Some(model) = models.into_iter().find(|v| v.capabilities.contains(capabilities)) {
                    client.set_model(model);
                } else {
                    anyhow::bail!(
                        "The current model is incapable of doing that."
                    );
                }
            }
            Ok(())
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
        fn config(
            &self,
        ) -> (
            &$crate::config::GlobalConfig,
            &Option<$crate::client::ExtraConfig>,
        ) {
            (&self.global_config, &self.config.extra)
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
                data: $crate::client::SendData,
            ) -> anyhow::Result<(String, $crate::client::CompletionDetails)> {
                let builder = self.request_builder(client, data)?;
                $send_message(builder).await
            }

            async fn send_message_streaming_inner(
                &self,
                client: &reqwest::Client,
                handler: &mut $crate::client::SseHandler,
                data: $crate::client::SendData,
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
    fn config(&self) -> (&GlobalConfig, &Option<ExtraConfig>);

    fn list_models(&self) -> Vec<Model>;

    fn name(&self) -> &str;

    fn model(&self) -> &Model;

    fn model_mut(&mut self) -> &mut Model;

    fn set_model(&mut self, model: Model);

    fn build_client(&self) -> Result<ReqwestClient> {
        let mut builder = ReqwestClient::builder();
        let options = self.config().1;
        let timeout = options
            .as_ref()
            .and_then(|v| v.connect_timeout)
            .unwrap_or(10);
        let proxy = options.as_ref().and_then(|v| v.proxy.clone());
        builder = set_proxy(builder, &proxy)?;
        let client = builder
            .connect_timeout(Duration::from_secs(timeout))
            .build()
            .with_context(|| "Failed to build client")?;
        Ok(client)
    }

    async fn send_message(&self, input: Input) -> Result<(String, CompletionDetails)> {
        let global_config = self.config().0;
        if global_config.read().dry_run {
            let content = global_config.read().echo_messages(&input);
            return Ok((content, CompletionDetails::default()));
        }
        let client = self.build_client()?;
        let data = global_config.read().prepare_send_data(&input, false)?;
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
                let global_config = self.config().0;
                if global_config.read().dry_run {
                    let content = global_config.read().echo_messages(&input);
                    let tokens = tokenize(&content);
                    for token in tokens {
                        tokio::time::sleep(Duration::from_millis(10)).await;
                        handler.text(&token)?;
                    }
                    return Ok(());
                }
                let client = self.build_client()?;
                let data = global_config.read().prepare_send_data(&input, true)?;
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

    async fn send_message_inner(
        &self,
        client: &ReqwestClient,
        data: SendData,
    ) -> Result<(String, CompletionDetails)>;

    async fn send_message_streaming_inner(
        &self,
        client: &ReqwestClient,
        handler: &mut SseHandler,
        data: SendData,
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

#[derive(Debug)]
pub struct SendData {
    pub messages: Vec<Message>,
    pub temperature: Option<f64>,
    pub top_p: Option<f64>,
    pub stream: bool,
}

#[derive(Debug, Clone, Default)]
pub struct CompletionDetails {
    pub id: Option<String>,
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
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
) -> Result<String> {
    let (tx, rx) = unbounded_channel();
    let mut stream_handler = SseHandler::new(tx, abort.clone());

    let (send_ret, rend_ret) = tokio::join!(
        client.send_message_streaming(input, &mut stream_handler),
        render_stream(rx, config, abort.clone()),
    );
    if let Err(err) = rend_ret {
        render_error(err, config.read().highlight);
    }
    let output = stream_handler.get_buffer().to_string();
    match send_ret {
        Ok(_) => {
            println!();
            Ok(output)
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
        if let (Some(typ), Some(message)) = (error["type"].as_str(), error["message"].as_str()) {
            bail!("{message} (type: {typ})");
        }
    } else if let Some(error) = data["errors"][0].as_object() {
        if let (Some(code), Some(message)) = (error["code"].as_u64(), error["message"].as_str()) {
            bail!("{message} (status: {code})")
        }
    } else if let Some(error) = data[0]["error"].as_object() {
        if let (Some(status), Some(message)) = (error["status"].as_str(), error["message"].as_str())
        {
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

#[derive(Debug)]
pub struct SsMmessage {
    pub event: String,
    pub data: String,
}

pub async fn sse_stream<F>(builder: RequestBuilder, mut handle: F) -> Result<()>
where
    F: FnMut(SsMmessage) -> Result<bool>,
{
    let mut es = builder.eventsource()?;
    while let Some(event) = es.next().await {
        match event {
            Ok(Event::Open) => {}
            Ok(Event::Message(message)) => {
                let message = SsMmessage {
                    event: message.event,
                    data: message.data,
                };
                if handle(message)? {
                    break;
                }
            }
            Err(err) => {
                match err {
                    EventSourceError::StreamEnded => {}
                    EventSourceError::InvalidStatusCode(status, res) => {
                        let text = res.text().await?;
                        let data: Value = match text.parse() {
                            Ok(data) => data,
                            Err(_) => {
                                bail!(
                                    "Invalid response data: {text} (status: {})",
                                    status.as_u16()
                                );
                            }
                        };
                        catch_error(&data, status.as_u16())?;
                    }
                    EventSourceError::InvalidContentType(header_value, res) => {
                        let text = res.text().await?;
                        bail!(
                            "Invalid response event-stream. content-type: {}, data: {text}",
                            header_value.to_str().unwrap_or_default()
                        );
                    }
                    _ => {
                        bail!("{}", err);
                    }
                }
                es.close();
            }
        }
    }
    Ok(())
}

pub async fn json_stream<S, F>(mut stream: S, mut handle: F) -> Result<()>
where
    S: Stream<Item = Result<bytes::Bytes, reqwest::Error>> + Unpin,
    F: FnMut(&str) -> Result<()>,
{
    let mut buffer = vec![];
    let mut cursor = 0;
    let mut start = 0;
    let mut balances = vec![];
    let mut quoting = false;
    let mut escape = false;
    while let Some(chunk) = stream.next().await {
        let chunk = chunk?;
        let chunk = std::str::from_utf8(&chunk)?;
        buffer.extend(chunk.chars());
        for i in cursor..buffer.len() {
            let ch = buffer[i];
            if quoting {
                if ch == '\\' {
                    escape = !escape;
                } else {
                    if !escape && ch == '"' {
                        quoting = false;
                    }
                    escape = false;
                }
                continue;
            }
            match ch {
                '"' => {
                    quoting = true;
                    escape = false;
                }
                '{' => {
                    if balances.is_empty() {
                        start = i;
                    }
                    balances.push(ch);
                }
                '[' => {
                    if start != 0 {
                        balances.push(ch);
                    }
                }
                '}' => {
                    balances.pop();
                    if balances.is_empty() {
                        let value: String = buffer[start..=i].iter().collect();
                        handle(&value)?;
                    }
                }
                ']' => {
                    balances.pop();
                }
                _ => {}
            }
        }
        cursor = buffer.len();
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
