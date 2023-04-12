use anyhow::{anyhow, Result};
use async_openai::{
    types::{
        ChatCompletionRequestMessageArgs, CreateChatCompletionRequest,
        CreateChatCompletionRequestArgs,
    },
    Client as OpenAiClient,
};
use futures_util::StreamExt;
use reqwest::{Client as ReqwestClient, Proxy as ReqwestProxy};

use crate::{config::SharedConfig, repl::ReplyStreamHandler};

pub struct ChatGptClient {
    config: SharedConfig,
    open_ai_client: OpenAiClient,
}

impl ChatGptClient {
    pub fn init(config: SharedConfig) -> Result<Self> {
        // set up http client
        let mut http_client =
            ReqwestClient::builder().connect_timeout(config.read().get_connect_timeout());

        if let Some(proxy) = config.read().proxy.as_ref() {
            http_client = http_client.proxy(ReqwestProxy::all(proxy)?);
        }

        // set up openai client
        let (api_key, organization_id) = config.read().get_api_key();

        let mut open_ai_client = OpenAiClient::new()
            .with_api_key(api_key)
            .with_http_client(http_client.build()?);

        if let Some(org_id) = organization_id {
            open_ai_client = open_ai_client.with_org_id(org_id);
        }

        Ok(Self {
            config,
            open_ai_client,
        })
    }

    pub async fn send_message(&self, content: &str) -> Result<String> {
        if self.config.read().dry_run {
            return Ok(self.config.read().echo_messages(content));
        }

        let request = self.request_builder(content, false)?;
        let response = self.open_ai_client.chat().create(request).await?;

        let output = response
            .choices
            .first()
            .ok_or_else(|| anyhow!("unexpected response"))?;

        Ok(output.message.content.clone())
    }

    pub async fn send_message_streaming(
        &self,
        content: &str,
        handler: &mut ReplyStreamHandler,
    ) -> Result<()> {
        if self.config.read().dry_run {
            handler.text(&self.config.read().echo_messages(content))?;

            return Ok(());
        }

        let abort = handler.get_abort();
        let request = self.request_builder(content, true)?;

        let mut stream = self.open_ai_client.chat().create_stream(request).await?;

        while let Some(result) = stream.next().await {
            if abort.aborted() {
                break;
            }

            match result {
                Ok(response) => {
                    if let Some(content) = &response
                        .choices
                        .first()
                        .expect("failed to extract streaming results")
                        .delta
                        .content
                    {
                        handler.text(content)?;
                    }
                }
                Err(err) => {
                    return Err(anyhow!("error streaming messages {err}"));
                }
            }
        }

        handler.done()?;

        Ok::<(), anyhow::Error>(())
    }

    fn request_builder(&self, content: &str, stream: bool) -> Result<CreateChatCompletionRequest> {
        // when i set max_tokens it wasnt working
        let (model, _max_tokens) = self.config.read().get_model();

        // default temperature is 1.0 (valid values between 0 and 2)
        // https://platform.openai.com/docs/api-reference/chat/create
        let temperature = self.config.read().get_temperature().unwrap_or(1.0) as f32;

        // build messages
        let messages: Vec<_> = self
            .config
            .read()
            .build_messages(content)?
            .iter()
            .map(|m| {
                ChatCompletionRequestMessageArgs::default()
                    .role(m.role.clone())
                    .content(m.content.clone())
                    .build()
                    .expect("failed to build message")
            })
            .collect();

        Ok(CreateChatCompletionRequestArgs::default()
            .model(model)
            .temperature(temperature)
            .stream(stream)
            .messages(messages)
            .build()?)
    }
}
