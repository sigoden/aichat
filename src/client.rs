use crate::config::SharedConfig;
use crate::repl::{ReplyStreamHandler, SharedAbortSignal};

use anyhow::{anyhow, Context, Result};
use eventsource_stream::Eventsource;
use futures_util::StreamExt;
use reqwest::{Client, Proxy, RequestBuilder};
use serde_json::{json, Value};
use std::time::Duration;
use tokio::runtime::Runtime;
use tokio::time::sleep;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const API_URL: &str = "https://api.openai.com/v1/chat/completions";
const MODEL: &str = "gpt-3.5-turbo";

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

    pub fn send_message(&self, input: &str, prompt: Option<String>) -> Result<String> {
        self.runtime.block_on(async {
            self.send_message_inner(input, prompt)
                .await
                .with_context(|| "Failed to send message")
        })
    }

    pub fn send_message_streaming(
        &self,
        input: &str,
        prompt: Option<String>,
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
                ret = self.send_message_streaming_inner(input, prompt, handler) => {
                    handler.done()?;
                    ret.with_context(|| "Failed to send message streaming")
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

    async fn send_message_inner(&self, content: &str, prompt: Option<String>) -> Result<String> {
        if self.config.borrow().dry_run {
            return Ok(combine(content, prompt));
        }
        let builder = self.request_builder(content, prompt, false)?;

        let data: Value = builder.send().await?.json().await?;

        let output = data["choices"][0]["message"]["content"]
            .as_str()
            .ok_or_else(|| anyhow!("Unexpected response {data}"))?;

        Ok(output.to_string())
    }

    async fn send_message_streaming_inner(
        &self,
        content: &str,
        prompt: Option<String>,
        handler: &mut ReplyStreamHandler,
    ) -> Result<()> {
        if self.config.borrow().dry_run {
            handler.text(&combine(content, prompt))?;
            return Ok(());
        }
        let builder = self.request_builder(content, prompt, true)?;
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
                handler.text(text)?;
            }
        }

        Ok(())
    }

    fn build_client(&self) -> Result<Client> {
        let mut builder = Client::builder();
        if let Some(proxy) = self.config.borrow().proxy.as_ref() {
            builder = builder.proxy(Proxy::all(proxy).with_context(|| "Invalid config.proxy")?);
        }
        let client = builder
            .connect_timeout(CONNECT_TIMEOUT)
            .build()
            .with_context(|| "Failed to build http client")?;
        Ok(client)
    }

    fn request_builder(
        &self,
        content: &str,
        prompt: Option<String>,
        stream: bool,
    ) -> Result<RequestBuilder> {
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

        if let Some(v) = self.config.borrow().temperature {
            body.as_object_mut()
                .and_then(|m| m.insert("temperature".into(), json!(v)));
        }

        if stream {
            body.as_object_mut()
                .and_then(|m| m.insert("stream".into(), json!(true)));
        }

        let builder = self
            .build_client()?
            .post(API_URL)
            .bearer_auth(&self.config.borrow().api_key)
            .json(&body);

        Ok(builder)
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
        .with_context(|| "Failed to init tokio")
}
