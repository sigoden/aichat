use crate::config::Config;
use crate::repl::ReplyReceiver;

use anyhow::{anyhow, Result};
use eventsource_stream::Eventsource;
use futures_util::StreamExt;
use reqwest::{Client, Proxy, RequestBuilder};
use serde_json::{json, Value};
use std::sync::atomic::{AtomicBool, Ordering};
use std::{sync::Arc, time::Duration};
use tokio::runtime::Runtime;
use tokio::time::sleep;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const API_URL: &str = "https://api.openai.com/v1/chat/completions";
const MODEL: &str = "gpt-3.5-turbo";

#[derive(Debug)]
pub struct ChatGptClient {
    client: Client,
    config: Arc<Config>,
    runtime: Runtime,
}

impl ChatGptClient {
    pub fn init(config: Arc<Config>) -> Result<Self> {
        let mut builder = Client::builder();
        if let Some(proxy) = config.proxy.as_ref() {
            builder = builder
                .proxy(Proxy::all(proxy).map_err(|err| anyhow!("Invalid config.proxy, {err}"))?);
        }
        let client = builder
            .connect_timeout(CONNECT_TIMEOUT)
            .build()
            .map_err(|err| anyhow!("Failed to init http client, {err}"))?;

        let runtime = init_runtime()?;
        Ok(Self {
            client,
            config,
            runtime,
        })
    }

    pub fn acquire(&self, input: &str, prompt: Option<String>) -> Result<String> {
        self.runtime
            .block_on(async { self.acquire_inner(input, prompt).await })
    }

    pub fn acquire_stream(
        &self,
        input: &str,
        prompt: Option<String>,
        receiver: &mut ReplyReceiver,
        ctrlc: Arc<AtomicBool>,
    ) -> Result<()> {
        async fn watch_ctrlc(ctrlc: Arc<AtomicBool>) {
            loop {
                if ctrlc.load(Ordering::SeqCst) {
                    break;
                }
                sleep(Duration::from_millis(100)).await;
            }
        }
        self.runtime.block_on(async {
            tokio::select! {
                ret = self.acquire_stream_inner(input, prompt, receiver) => {
                    receiver.done();
                    ret
                }
                _ = watch_ctrlc(ctrlc.clone()) => {
                    receiver.done();
                    Ok(())
                 },
                _ =  tokio::signal::ctrl_c() => {
                    ctrlc.store(true, Ordering::SeqCst);
                    Ok(())
                }
            }
        })
    }

    async fn acquire_inner(&self, content: &str, prompt: Option<String>) -> Result<String> {
        if self.config.dry_run {
            return Ok(combine(content, prompt));
        }
        let builder = self.request_builder(content, prompt, false);

        let data: Value = builder.send().await?.json().await?;

        let output = data["choices"][0]["message"]["content"]
            .as_str()
            .ok_or_else(|| anyhow!("Unexpected response {data}"))?;

        Ok(output.to_string())
    }

    async fn acquire_stream_inner(
        &self,
        content: &str,
        prompt: Option<String>,
        receiver: &mut ReplyReceiver,
    ) -> Result<()> {
        if self.config.dry_run {
            receiver.text(&combine(content, prompt));
            return Ok(());
        }
        let builder = self.request_builder(content, prompt, true);
        let mut stream = builder.send().await?.bytes_stream().eventsource();
        let mut virgin = true;
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
                if virgin {
                    virgin = false;
                    if text == "\n\n" {
                        continue;
                    }
                }
                receiver.text(text);
            }
        }

        Ok(())
    }

    fn request_builder(
        &self,
        content: &str,
        prompt: Option<String>,
        stream: bool,
    ) -> RequestBuilder {
        let user_message = json!({ "role": "user", "content": content });
        let messages = match prompt {
            Some(prompt) => {
                let system_message = json!({ "role": "system", "content": prompt.trim() });
                json!([system_message, user_message])
            }
            None => {
                json!([user_message])
            }
        };
        let mut body = json!({
            "model": MODEL,
            "messages": messages,
        });

        if let Some(v) = self.config.temperature {
            body.as_object_mut()
                .and_then(|m| m.insert("temperature".into(), json!(v)));
        }

        if stream {
            body.as_object_mut()
                .and_then(|m| m.insert("stream".into(), json!(true)));
        }

        self.client
            .post(API_URL)
            .bearer_auth(&self.config.api_key)
            .json(&body)
    }
}

fn combine(content: &str, prompt: Option<String>) -> String {
    match prompt {
        Some(prompt) => format!("{}\n{content}", prompt.trim()),
        None => content.to_string(),
    }
}
fn init_runtime() -> Result<Runtime> {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|err| anyhow!("Failed to init tokio, {err}"))
}
