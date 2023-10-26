pub mod localai;
pub mod openai;

use self::{
    localai::LocalAIConfig,
    openai::{OpenAIClient, OpenAIConfig},
};

use anyhow::{bail, Context, Result};
use async_trait::async_trait;
use serde::Deserialize;
use std::time::Duration;
use tokio::runtime::Runtime;
use tokio::time::sleep;

use crate::{
    client::localai::LocalAIClient,
    config::{Config, SharedConfig},
    repl::{ReplyStreamHandler, SharedAbortSignal},
};

#[allow(clippy::struct_excessive_bools)]
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type")]
pub enum ClientConfig {
    #[serde(rename = "openai")]
    OpenAI(OpenAIConfig),
    #[serde(rename = "localai")]
    LocalAI(LocalAIConfig),
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
        let client = OpenAIClient::name();
        let (name, max_tokens) = &OpenAIClient::list_models(&OpenAIConfig::default())[0];
        Self::new(client, name, *max_tokens, 0)
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

#[async_trait]
pub trait Client {
    fn get_config(&self) -> &SharedConfig;

    fn get_runtime(&self) -> &Runtime;

    fn send_message(&self, content: &str) -> Result<String> {
        self.get_runtime().block_on(async {
            if self.get_config().read().dry_run {
                return Ok(self.get_config().read().echo_messages(content));
            }
            self.send_message_inner(content)
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
        self.get_runtime().block_on(async {
            tokio::select! {
                ret = async {
                    if self.get_config().read().dry_run {
                        handler.text(&self.get_config().read().echo_messages(content))?;
                        return Ok(());
                    }
                    self.send_message_streaming_inner(content, handler).await
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

    async fn send_message_inner(&self, content: &str) -> Result<String>;

    async fn send_message_streaming_inner(
        &self,
        content: &str,
        handler: &mut ReplyStreamHandler,
    ) -> Result<()>;
}

pub fn init_client(config: SharedConfig, runtime: Runtime) -> Result<Box<dyn Client>> {
    let model_info = config.read().model_info.clone();
    let model_info_err = |model_info: &ModelInfo| {
        bail!(
            "Unknown client {} at config.clients[{}]",
            &model_info.client,
            &model_info.index
        )
    };
    if model_info.client == OpenAIClient::name() {
        let local_config = {
            if let ClientConfig::OpenAI(c) = &config.read().clients[model_info.index] {
                c.clone()
            } else {
                return model_info_err(&model_info);
            }
        };
        Ok(Box::new(OpenAIClient::new(
            config,
            local_config,
            model_info,
            runtime,
        )))
    } else if model_info.client == LocalAIClient::name() {
        let local_config = {
            if let ClientConfig::LocalAI(c) = &config.read().clients[model_info.index] {
                c.clone()
            } else {
                return model_info_err(&model_info);
            }
        };
        Ok(Box::new(LocalAIClient::new(
            config,
            local_config,
            model_info,
            runtime,
        )))
    } else {
        bail!("Unknown client {}", &model_info.client)
    }
}

pub fn all_clients() -> Vec<&'static str> {
    vec![OpenAIClient::name(), LocalAIClient::name()]
}

pub fn create_client_config(client: &str) -> Result<String> {
    if client == OpenAIClient::name() {
        OpenAIClient::create_config()
    } else if client == LocalAIClient::name() {
        LocalAIClient::create_config()
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
            ClientConfig::OpenAI(c) => OpenAIClient::list_models(c)
                .iter()
                .map(|(x, y)| ModelInfo::new(OpenAIClient::name(), x, *y, i))
                .collect::<Vec<ModelInfo>>(),
            ClientConfig::LocalAI(c) => LocalAIClient::list_models(c)
                .iter()
                .map(|(x, y)| ModelInfo::new(LocalAIClient::name(), x, *y, i))
                .collect::<Vec<ModelInfo>>(),
        })
        .collect()
}
