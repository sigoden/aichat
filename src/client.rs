use crate::config::SharedConfig;
use crate::repl::{ReplyStreamHandler, SharedAbortSignal};

use anyhow::{anyhow, bail, Context, Result};
use eventsource_stream::Eventsource;
use futures_util::StreamExt;
use reqwest::{Client, Proxy, RequestBuilder};
use serde_json::{json, Value};
use std::time::Duration;
use tokio::runtime::Runtime;
use tokio::time::sleep;

const API_URL: &str = "https://api.openai.com/v1/chat/completions";

#[derive(Debug)]
pub struct ChatGptClient {
    config: SharedConfig,
    runtime: Runtime,
}

impl ChatGptClient {
    pub fn init(config: SharedConfig) -> Result<Self> {
        let runtime = init_runtime()?;
        let s = Self { config, runtime };
        let _ = s.build_client()?; // check error
        Ok(s)
    }

    pub fn send_message(&self, input: &str) -> Result<String> {
        self.runtime.block_on(async {
            self.send_message_inner(input)
                .await
                .with_context(|| "Failed to fetch")
        })
    }

    pub fn send_message_streaming(
        &self,
        input: &str,
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
        self.runtime.block_on(async {
            tokio::select! {
                ret = self.send_message_streaming_inner(input, handler) => {
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

    async fn send_message_inner(&self, content: &str) -> Result<String> {
        if self.config.read().dry_run {
            return Ok(self.config.read().echo_messages(content));
        }
        let builder = self.request_builder(content, false)?;
        let data: Value = builder.send().await?.json().await?;
        if let Some(err_msg) = data["error"]["message"].as_str() {
            bail!("Request failed, {err_msg}");
        }

        let output = data["choices"][0]["message"]["content"]
            .as_str()
            .ok_or_else(|| anyhow!("Unexpected response {data}"))?;

        Ok(output.to_string())
    }

    async fn send_message_streaming_inner(
        &self,
        content: &str,
        handler: &mut ReplyStreamHandler,
    ) -> Result<()> {
        if self.config.read().dry_run {
            handler.text(&self.config.read().echo_messages(content))?;
            return Ok(());
        }
        let builder = self.request_builder(content, true)?;
        let res = builder.send().await?;
        if !res.status().is_success() {
            let data: Value = res.json().await?;
            if let Some(err_msg) = data["error"]["message"].as_str() {
                bail!("Request failed, {err_msg}");
            }
            bail!("Request failed");
        }
        let mut stream = res.bytes_stream().eventsource();
        while let Some(part) = stream.next().await {
            let chunk = part?.data;
            if chunk == "[DONE]" {
                break;
            } else {
                let data: Value = serde_json::from_str(&chunk)?;
                let text = data["choices"][0]["delta"]["content"]
                    .as_str()
                    .unwrap_or_default();
                if text.is_empty() {
                    continue;
                }
                handler.text(text)?;
            }
        }

        Ok(())
    }

    fn build_client(&self) -> Result<Client> {
        let mut builder = Client::builder();
        if let Some(proxy) = self.config.read().proxy.as_ref() {
            builder = builder
                .proxy(Proxy::all(proxy).with_context(|| format!("Invalid proxy `{proxy}`"))?);
        }
        let timeout = self.config.read().get_connect_timeout();
        let client = builder
            .connect_timeout(timeout)
            .build()
            .with_context(|| "Failed to build http client")?;
        Ok(client)
    }

    fn request_builder(&self, content: &str, stream: bool) -> Result<RequestBuilder> {
        let (model, _) = self.config.read().get_model();
        let messages = self.config.read().build_messages(content)?;
        let mut body = json!({
            "model": model,
            "messages": messages,
        });

        if let Some(v) = self.config.read().get_temperature() {
            body.as_object_mut()
                .and_then(|m| m.insert("temperature".into(), json!(v)));
        }

        if stream {
            body.as_object_mut()
                .and_then(|m| m.insert("stream".into(), json!(true)));
        }

        let (api_key, organization_id) = self.config.read().get_api_key();

        let mut builder = self
            .build_client()?
            .post(API_URL)
            .bearer_auth(api_key)
            .json(&body);

        if let Some(organization_id) = organization_id {
            builder = builder.header("OpenAI-Organization", organization_id);
        }

        Ok(builder)
    }
}

fn init_runtime() -> Result<Runtime> {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .with_context(|| "Failed to init tokio")
}
