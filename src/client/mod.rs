pub mod azure_openai;
pub mod localai;
pub mod openai;

use self::{
    azure_openai::{AzureOpenAIClient, AzureOpenAIConfig},
    localai::LocalAIConfig,
    openai::{OpenAIClient, OpenAIConfig},
};

use crate::{
    client::localai::LocalAIClient,
    config::{Config, Message, SharedConfig},
    repl::{ReplyStreamHandler, SharedAbortSignal},
    utils::tokenize,
};

use anyhow::{anyhow, bail, Context, Result};
use async_trait::async_trait;
use inquire::{required, Text};
use reqwest::{Client as ReqwestClient, ClientBuilder, Proxy};
use serde::Deserialize;
use std::{env, time::Duration};
use tokio::time::sleep;

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type")]
pub enum ClientConfig {
    #[serde(rename = "openai")]
    OpenAI(OpenAIConfig),
    #[serde(rename = "localai")]
    LocalAI(LocalAIConfig),
    #[serde(rename = "azure-openai")]
    AzureOpenAI(AzureOpenAIConfig),
}
#[derive(Debug, Clone)]
pub struct ModelInfo {
    pub client: String,
    pub name: String,
    pub max_tokens: usize,
    pub index: usize,
}

impl Default for ModelInfo {
    fn default() -> Self {
        OpenAIClient::list_models(&OpenAIConfig::default(), 0)[0].clone()
    }
}

impl ModelInfo {
    pub fn new(client: &str, name: &str, max_tokens: usize, index: usize) -> Self {
        Self {
            client: client.into(),
            name: name.into(),
            max_tokens,
            index,
        }
    }
    pub fn stringify(&self) -> String {
        format!("{}:{}", self.client, self.name)
    }
}

#[derive(Debug)]
pub struct SendData {
    pub messages: Vec<Message>,
    pub temperature: Option<f64>,
    pub stream: bool,
}
#[async_trait]
pub trait Client {
    fn config(&self) -> &SharedConfig;

    fn extra_config(&self) -> &Option<ExtraConfig>;

    fn build_client(&self) -> Result<ReqwestClient> {
        let mut builder = ReqwestClient::builder();
        let options = self.extra_config();
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

    fn send_message(&self, content: &str) -> Result<String> {
        init_tokio_runtime()?.block_on(async {
            if self.config().read().dry_run {
                let content = self.config().read().echo_messages(content);
                return Ok(content);
            }
            let client = self.build_client()?;
            let data = self.config().read().prepare_send_data(content, false)?;
            self.send_message_inner(&client, data)
                .await
                .with_context(|| "Failed to fetch")
        })
    }

    fn send_message_streaming(
        &self,
        content: &str,
        handler: &mut ReplyStreamHandler,
    ) -> Result<()> {
        async fn watch_abort(abort: SharedAbortSignal) {
            loop {
                if abort.aborted() {
                    break;
                }
                sleep(Duration::from_millis(100)).await;
            }
        }
        let abort = handler.get_abort();
        init_tokio_runtime()?.block_on(async {
            tokio::select! {
                ret = async {
                    if self.config().read().dry_run {
                        let content = self.config().read().echo_messages(content);
                        let tokens = tokenize(&content);
                        for token in tokens {
                            tokio::time::sleep(Duration::from_millis(25)).await;
                            handler.text(&token)?;
                        }
                        return Ok(());
                    }
                    let client = self.build_client()?;
                    let data = self.config().read().prepare_send_data(content, true)?;
                    self.send_message_streaming_inner(&client, handler, data).await
                } => {
                    handler.done()?;
                    ret.with_context(|| "Failed to fetch stream")
                }
                _ = watch_abort(abort.clone()) => {
                    handler.done()?;
                    Ok(())
                 },
                _ =  tokio::signal::ctrl_c() => {
                    abort.set_ctrlc();
                    Ok(())
                }
            }
        })
    }

    async fn send_message_inner(&self, client: &ReqwestClient, data: SendData) -> Result<String>;

    async fn send_message_streaming_inner(
        &self,
        client: &ReqwestClient,
        handler: &mut ReplyStreamHandler,
        data: SendData,
    ) -> Result<()>;
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct ExtraConfig {
    pub proxy: Option<String>,
    pub connect_timeout: Option<u64>,
}

pub fn init_client(config: SharedConfig) -> Result<Box<dyn Client>> {
    OpenAIClient::init(config.clone())
        .or_else(|| LocalAIClient::init(config.clone()))
        .or_else(|| AzureOpenAIClient::init(config.clone()))
        .ok_or_else(|| {
            let model_info = config.read().model_info.clone();
            anyhow!(
                "Unknown client {} at config.clients[{}]",
                &model_info.client,
                &model_info.index
            )
        })
}

pub fn list_client_types() -> Vec<&'static str> {
    vec![
        OpenAIClient::NAME,
        LocalAIClient::NAME,
        AzureOpenAIClient::NAME,
    ]
}

pub fn create_client_config(client: &str) -> Result<String> {
    if client == OpenAIClient::NAME {
        OpenAIClient::create_config()
    } else if client == LocalAIClient::NAME {
        LocalAIClient::create_config()
    } else if client == AzureOpenAIClient::NAME {
        AzureOpenAIClient::create_config()
    } else {
        bail!("Unknown client {}", &client)
    }
}

pub fn list_models(config: &Config) -> Vec<ModelInfo> {
    config
        .clients
        .iter()
        .enumerate()
        .flat_map(|(i, v)| match v {
            ClientConfig::OpenAI(c) => OpenAIClient::list_models(c, i),
            ClientConfig::LocalAI(c) => LocalAIClient::list_models(c, i),
            ClientConfig::AzureOpenAI(c) => AzureOpenAIClient::list_models(c, i),
        })
        .collect()
}

pub(crate) fn init_tokio_runtime() -> Result<tokio::runtime::Runtime> {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .with_context(|| "Failed to init tokio")
}

pub(crate) fn prompt_input_api_base() -> Result<String> {
    Text::new("API Base:")
        .with_validator(required!("This field is required"))
        .prompt()
        .map_err(prompt_op_err)
}

pub(crate) fn prompt_input_api_key() -> Result<String> {
    Text::new("API Key:")
        .with_validator(required!("This field is required"))
        .prompt()
        .map_err(prompt_op_err)
}

pub(crate) fn prompt_input_api_key_optional() -> Result<String> {
    Text::new("API Key:").prompt().map_err(prompt_op_err)
}

pub(crate) fn prompt_input_model_name() -> Result<String> {
    Text::new("Model Name:")
        .with_validator(required!("This field is required"))
        .prompt()
        .map_err(prompt_op_err)
}

pub(crate) fn prompt_input_max_token() -> Result<String> {
    Text::new("Max tokens:")
        .with_default("4096")
        .with_validator(required!("This field is required"))
        .prompt()
        .map_err(prompt_op_err)
}

pub(crate) fn prompt_op_err<T>(_: T) -> anyhow::Error {
    anyhow!("An error happened, try again later.")
}

fn set_proxy(builder: ClientBuilder, proxy: &Option<String>) -> Result<ClientBuilder> {
    let proxy = if let Some(proxy) = proxy {
        if proxy.is_empty() || proxy == "false" || proxy == "-" {
            return Ok(builder);
        }
        proxy.clone()
    } else if let Ok(proxy) = env::var("HTTPS_PROXY").or_else(|_| env::var("ALL_PROXY")) {
        proxy
    } else {
        return Ok(builder);
    };
    let builder =
        builder.proxy(Proxy::all(&proxy).with_context(|| format!("Invalid proxy `{proxy}`"))?);
    Ok(builder)
}
