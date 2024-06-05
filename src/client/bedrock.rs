use super::claude::*;
use super::prompt_format::*;
use super::*;

use crate::utils::{base64_decode, encode_uri, hex_encode, hmac_sha256, sha256};

use anyhow::{anyhow, bail, Result};
use async_trait::async_trait;
use aws_smithy_eventstream::frame::{DecodedFrame, MessageFrameDecoder};
use aws_smithy_eventstream::smithy::parse_response_headers;
use bytes::BytesMut;
use chrono::{DateTime, Utc};
use futures_util::StreamExt;
use indexmap::IndexMap;
use reqwest::{
    header::{HeaderMap, HeaderName, HeaderValue},
    Client as ReqwestClient, Method, RequestBuilder,
};
use serde::Deserialize;
use serde_json::{json, Value};
use std::str::FromStr;

#[derive(Debug, Clone, Deserialize)]
pub struct BedrockConfig {
    pub name: Option<String>,
    pub access_key_id: Option<String>,
    pub secret_access_key: Option<String>,
    pub region: Option<String>,
    #[serde(default)]
    pub models: Vec<ModelData>,
    pub patches: Option<ModelPatches>,
    pub extra: Option<ExtraConfig>,
}

#[async_trait]
impl Client for BedrockClient {
    client_common_fns!();

    async fn chat_completions_inner(
        &self,
        client: &ReqwestClient,
        data: ChatCompletionsData,
    ) -> Result<ChatCompletionsOutput> {
        let model_category = ModelCategory::from_str(self.model.name())?;
        let builder = self.chat_completions_builder(client, data, &model_category)?;
        chat_completions(builder, &model_category).await
    }

    async fn chat_completions_streaming_inner(
        &self,
        client: &ReqwestClient,
        handler: &mut SseHandler,
        data: ChatCompletionsData,
    ) -> Result<()> {
        let model_category = ModelCategory::from_str(self.model.name())?;
        let builder = self.chat_completions_builder(client, data, &model_category)?;
        chat_completions_streaming(builder, handler, &model_category).await
    }
}

impl BedrockClient {
    config_get_fn!(access_key_id, get_access_key_id);
    config_get_fn!(secret_access_key, get_secret_access_key);
    config_get_fn!(region, get_region);

    pub const PROMPTS: [PromptAction<'static>; 3] = [
        (
            "access_key_id",
            "AWS Access Key ID",
            true,
            PromptKind::String,
        ),
        (
            "secret_access_key",
            "AWS Secret Access Key",
            true,
            PromptKind::String,
        ),
        ("region", "AWS Region", true, PromptKind::String),
    ];

    fn chat_completions_builder(
        &self,
        client: &ReqwestClient,
        data: ChatCompletionsData,
        model_category: &ModelCategory,
    ) -> Result<RequestBuilder> {
        let access_key_id = self.get_access_key_id()?;
        let secret_access_key = self.get_secret_access_key()?;
        let region = self.get_region()?;

        let model_name = &self.model.name();
        let uri = if data.stream {
            format!("/model/{model_name}/invoke-with-response-stream")
        } else {
            format!("/model/{model_name}/invoke")
        };
        let host = format!("bedrock-runtime.{region}.amazonaws.com");

        let headers = IndexMap::new();

        let mut body = build_chat_completions_body(data, &self.model, model_category)?;
        self.patch_chat_completions_body(&mut body);

        let builder = aws_fetch(
            client,
            &AwsCredentials {
                access_key_id,
                secret_access_key,
                region,
            },
            AwsRequest {
                method: Method::POST,
                host,
                service: "bedrock".into(),
                uri,
                querystring: "".into(),
                headers,
                body: body.to_string(),
            },
        )?;

        Ok(builder)
    }
}

async fn chat_completions(
    builder: RequestBuilder,
    model_category: &ModelCategory,
) -> Result<ChatCompletionsOutput> {
    let res = builder.send().await?;
    let status = res.status();
    let data: Value = res.json().await?;

    if !status.is_success() {
        catch_error(&data, status.as_u16())?;
    }

    debug!("non-stream-data: {data}");
    match model_category {
        ModelCategory::Anthropic => claude_extract_chat_completions(&data),
        ModelCategory::MetaLlama3 => llama_extract_chat_completions(&data),
        ModelCategory::Mistral => mistral_extract_chat_completions(&data),
    }
}

async fn chat_completions_streaming(
    builder: RequestBuilder,
    handler: &mut SseHandler,
    model_category: &ModelCategory,
) -> Result<()> {
    let res = builder.send().await?;
    let status = res.status();
    if !status.is_success() {
        let data: Value = res.json().await?;
        catch_error(&data, status.as_u16())?;
        bail!("Invalid response data: {data}");
    }
    let mut stream = res.bytes_stream();
    let mut buffer = BytesMut::new();
    let mut decoder = MessageFrameDecoder::new();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk?;
        buffer.extend_from_slice(&chunk);
        while let DecodedFrame::Complete(message) = decoder.decode_frame(&mut buffer)? {
            let response_headers = parse_response_headers(&message)?;
            let message_type = response_headers.message_type.as_str();
            let smithy_type = response_headers.smithy_type.as_str();
            match (message_type, smithy_type) {
                ("event", "chunk") => {
                    let data: Value = decode_chunk(message.payload()).ok_or_else(|| {
                        anyhow!("Invalid chunk data: {}", hex_encode(message.payload()))
                    })?;
                    debug!("stream-data: {data}");
                    match model_category {
                        ModelCategory::Anthropic => {
                            if let Some(typ) = data["type"].as_str() {
                                if typ == "content_block_delta" {
                                    if let Some(text) = data["delta"]["text"].as_str() {
                                        handler.text(text)?;
                                    }
                                }
                            }
                        }
                        ModelCategory::MetaLlama3 => {
                            if let Some(text) = data["generation"].as_str() {
                                handler.text(text)?;
                            }
                        }
                        ModelCategory::Mistral => {
                            if let Some(text) = data["outputs"][0]["text"].as_str() {
                                handler.text(text)?;
                            }
                        }
                    }
                }
                ("exception", _) => {
                    let payload = base64_decode(message.payload())?;
                    let data = String::from_utf8_lossy(&payload);

                    bail!("Invalid response data: {data} (smithy_type: {smithy_type})")
                }
                _ => {
                    bail!("Unrecognized message, message_type: {message_type}, smithy_type: {smithy_type}",);
                }
            }
        }
    }
    Ok(())
}

fn build_chat_completions_body(
    data: ChatCompletionsData,
    model: &Model,
    model_category: &ModelCategory,
) -> Result<Value> {
    match model_category {
        ModelCategory::Anthropic => {
            let mut body = claude_build_chat_completions_body(data, model)?;
            if let Some(body_obj) = body.as_object_mut() {
                body_obj.remove("model");
                body_obj.remove("stream");
            }
            body["anthropic_version"] = "bedrock-2023-05-31".into();
            Ok(body)
        }
        ModelCategory::MetaLlama3 => {
            meta_llama_build_chat_completions_body(data, model, LLAMA3_PROMPT_FORMAT)
        }
        ModelCategory::Mistral => mistral_build_chat_completions_body(data, model),
    }
}

fn meta_llama_build_chat_completions_body(
    data: ChatCompletionsData,
    model: &Model,
    pt: PromptFormat,
) -> Result<Value> {
    let ChatCompletionsData {
        messages,
        temperature,
        top_p,
        functions: _,
        stream: _,
    } = data;
    let prompt = generate_prompt(&messages, pt)?;
    let mut body = json!({ "prompt": prompt });

    if let Some(v) = model.max_tokens_param() {
        body["max_gen_len"] = v.into();
    }
    if let Some(v) = temperature {
        body["temperature"] = v.into();
    }
    if let Some(v) = top_p {
        body["top_p"] = v.into();
    }

    Ok(body)
}

fn mistral_build_chat_completions_body(data: ChatCompletionsData, model: &Model) -> Result<Value> {
    let ChatCompletionsData {
        messages,
        temperature,
        top_p,
        functions: _,
        stream: _,
    } = data;
    let prompt = generate_prompt(&messages, MISTRAL_PROMPT_FORMAT)?;
    let mut body = json!({ "prompt": prompt });

    if let Some(v) = model.max_tokens_param() {
        body["max_tokens"] = v.into();
    }
    if let Some(v) = temperature {
        body["temperature"] = v.into();
    }
    if let Some(v) = top_p {
        body["top_p"] = v.into();
    }

    Ok(body)
}

fn llama_extract_chat_completions(data: &Value) -> Result<ChatCompletionsOutput> {
    let text = data["generation"]
        .as_str()
        .ok_or_else(|| anyhow!("Invalid response data: {data}"))?;
    let output = ChatCompletionsOutput {
        text: text.to_string(),
        tool_calls: vec![],
        id: None,
        input_tokens: data["prompt_token_count"].as_u64(),
        output_tokens: data["generation_token_count"].as_u64(),
    };
    Ok(output)
}

fn mistral_extract_chat_completions(data: &Value) -> Result<ChatCompletionsOutput> {
    let text = data["outputs"][0]["text"]
        .as_str()
        .ok_or_else(|| anyhow!("Invalid response data: {data}"))?;
    Ok(ChatCompletionsOutput::new(text))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ModelCategory {
    Anthropic,
    MetaLlama3,
    Mistral,
}

impl FromStr for ModelCategory {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        if s.starts_with("anthropic.") {
            Ok(ModelCategory::Anthropic)
        } else if s.starts_with("meta.llama3") {
            Ok(ModelCategory::MetaLlama3)
        } else if s.starts_with("mistral") {
            Ok(ModelCategory::Mistral)
        } else {
            unsupported_model!(s)
        }
    }
}

#[derive(Debug)]
struct AwsCredentials {
    access_key_id: String,
    secret_access_key: String,
    region: String,
}

#[derive(Debug)]
struct AwsRequest {
    method: Method,
    host: String,
    service: String,
    uri: String,
    querystring: String,
    headers: IndexMap<String, String>,
    body: String,
}

fn aws_fetch(
    client: &ReqwestClient,
    credentials: &AwsCredentials,
    request: AwsRequest,
) -> Result<RequestBuilder> {
    let AwsRequest {
        method,
        host,
        service,
        uri,
        querystring,
        mut headers,
        body,
    } = request;
    let region = &credentials.region;

    let endpoint = format!("https://{}{}", host, uri);

    let now: DateTime<Utc> = Utc::now();
    let amz_date = now.format("%Y%m%dT%H%M%SZ").to_string();
    let date_stamp = amz_date[0..8].to_string();
    headers.insert("host".into(), host.clone());
    headers.insert("x-amz-date".into(), amz_date.clone());

    let canonical_headers = headers
        .iter()
        .map(|(key, value)| format!("{}:{}\n", key, value))
        .collect::<Vec<_>>()
        .join("");

    let signed_headers = headers
        .iter()
        .map(|(key, _)| key.as_str())
        .collect::<Vec<_>>()
        .join(";");

    let payload_hash = sha256(&body);

    let canonical_request = format!(
        "{}\n{}\n{}\n{}\n{}\n{}",
        method,
        encode_uri(&uri),
        querystring,
        canonical_headers,
        signed_headers,
        payload_hash
    );

    let algorithm = "AWS4-HMAC-SHA256";
    let credential_scope = format!("{}/{}/{}/aws4_request", date_stamp, region, service);
    let string_to_sign = format!(
        "{}\n{}\n{}\n{}",
        algorithm,
        amz_date,
        credential_scope,
        sha256(&canonical_request)
    );

    let signing_key = gen_signing_key(
        &credentials.secret_access_key,
        &date_stamp,
        region,
        &service,
    );
    let signature = hmac_sha256(&signing_key, &string_to_sign);
    let signature = hex_encode(&signature);

    let authorization_header = format!(
        "{} Credential={}/{}, SignedHeaders={}, Signature={}",
        algorithm, credentials.access_key_id, credential_scope, signed_headers, signature
    );

    headers.insert("authorization".into(), authorization_header);

    let mut req_headers = HeaderMap::new();
    for (k, v) in &headers {
        req_headers.insert(HeaderName::from_str(k)?, HeaderValue::from_str(v)?);
    }

    debug!("Bedrock Request: {endpoint} {body}");

    let request_builder = client
        .request(method, endpoint)
        .headers(req_headers)
        .body(body);
    Ok(request_builder)
}

fn gen_signing_key(key: &str, date_stamp: &str, region: &str, service: &str) -> Vec<u8> {
    let k_date = hmac_sha256(format!("AWS4{}", key).as_bytes(), date_stamp);
    let k_region = hmac_sha256(&k_date, region);
    let k_service = hmac_sha256(&k_region, service);
    hmac_sha256(&k_service, "aws4_request")
}

fn decode_chunk(data: &[u8]) -> Option<Value> {
    let data = serde_json::from_slice::<Value>(data).ok()?;
    let data = data["bytes"].as_str()?;
    let data = base64_decode(data).ok()?;
    serde_json::from_slice(&data).ok()
}
