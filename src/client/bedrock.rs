use super::*;

use crate::utils::{base64_decode, encode_uri, hex_encode, hmac_sha256, sha256, strip_think_tag};

use anyhow::{bail, Context, Result};
use aws_smithy_eventstream::frame::{DecodedFrame, MessageFrameDecoder};
use aws_smithy_eventstream::smithy::parse_response_headers;
use bytes::BytesMut;
use chrono::{DateTime, Utc};
use futures_util::StreamExt;
use indexmap::IndexMap;
use reqwest::{Client as ReqwestClient, Method, RequestBuilder};
use serde::Deserialize;
use serde_json::{json, Value};

#[derive(Debug, Clone, Deserialize)]
pub struct BedrockConfig {
    pub name: Option<String>,
    pub access_key_id: Option<String>,
    pub secret_access_key: Option<String>,
    pub region: Option<String>,
    #[serde(default)]
    pub models: Vec<ModelData>,
    pub patch: Option<RequestPatch>,
    pub extra: Option<ExtraConfig>,
}

impl BedrockClient {
    config_get_fn!(access_key_id, get_access_key_id);
    config_get_fn!(secret_access_key, get_secret_access_key);
    config_get_fn!(region, get_region);

    pub const PROMPTS: [PromptAction<'static>; 3] = [
        ("access_key_id", "AWS Access Key ID", None),
        ("secret_access_key", "AWS Secret Access Key", None),
        ("region", "AWS Region", None),
    ];

    fn chat_completions_builder(
        &self,
        client: &ReqwestClient,
        data: ChatCompletionsData,
    ) -> Result<RequestBuilder> {
        let access_key_id = self.get_access_key_id()?;
        let secret_access_key = self.get_secret_access_key()?;
        let region = self.get_region()?;
        let host = format!("bedrock-runtime.{region}.amazonaws.com");

        let model_name = &self.model.real_name();

        let uri = if data.stream {
            format!("/model/{model_name}/converse-stream")
        } else {
            format!("/model/{model_name}/converse")
        };

        let body = build_chat_completions_body(data, &self.model)?;

        let mut request_data = RequestData::new("", body);
        self.patch_request_data(&mut request_data);
        let RequestData {
            url: _,
            headers,
            body,
        } = request_data;

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

    fn embeddings_builder(
        &self,
        client: &ReqwestClient,
        data: &EmbeddingsData,
    ) -> Result<RequestBuilder> {
        let access_key_id = self.get_access_key_id()?;
        let secret_access_key = self.get_secret_access_key()?;
        let region = self.get_region()?;
        let host = format!("bedrock-runtime.{region}.amazonaws.com");

        let uri = format!("/model/{}/invoke", self.model.real_name());

        let input_type = match data.query {
            true => "search_query",
            false => "search_document",
        };

        let body = json!({
            "texts": data.texts,
            "input_type": input_type,
        });

        let mut request_data = RequestData::new("", body);
        self.patch_request_data(&mut request_data);
        let RequestData {
            url: _,
            headers,
            body,
        } = request_data;

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

#[async_trait::async_trait]
impl Client for BedrockClient {
    client_common_fns!();

    async fn chat_completions_inner(
        &self,
        client: &ReqwestClient,
        data: ChatCompletionsData,
    ) -> Result<ChatCompletionsOutput> {
        let builder = self.chat_completions_builder(client, data)?;
        chat_completions(builder).await
    }

    async fn chat_completions_streaming_inner(
        &self,
        client: &ReqwestClient,
        handler: &mut SseHandler,
        data: ChatCompletionsData,
    ) -> Result<()> {
        let builder = self.chat_completions_builder(client, data)?;
        chat_completions_streaming(builder, handler).await
    }

    async fn embeddings_inner(
        &self,
        client: &ReqwestClient,
        data: &EmbeddingsData,
    ) -> Result<EmbeddingsOutput> {
        let builder = self.embeddings_builder(client, data)?;
        embeddings(builder).await
    }
}

async fn chat_completions(builder: RequestBuilder) -> Result<ChatCompletionsOutput> {
    let res = builder.send().await?;
    let status = res.status();
    let data: Value = res.json().await?;

    if !status.is_success() {
        catch_error(&data, status.as_u16())?;
    }

    debug!("non-stream-data: {data}");
    extract_chat_completions(&data)
}

async fn chat_completions_streaming(
    builder: RequestBuilder,
    handler: &mut SseHandler,
) -> Result<()> {
    let res = builder.send().await?;
    let status = res.status();
    if !status.is_success() {
        let data: Value = res.json().await?;
        catch_error(&data, status.as_u16())?;
        bail!("Invalid response data: {data}");
    }

    let mut function_name = String::new();
    let mut function_arguments = String::new();
    let mut function_id = String::new();
    let mut reasoning_state = 0;

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
                ("event", _) => {
                    let data: Value = serde_json::from_slice(message.payload())?;
                    debug!("stream-data: {smithy_type} {data}");
                    match smithy_type {
                        "contentBlockStart" => {
                            if let Some(tool_use) = data["start"]["toolUse"].as_object() {
                                if let (Some(id), Some(name)) = (
                                    json_str_from_map(tool_use, "toolUseId"),
                                    json_str_from_map(tool_use, "name"),
                                ) {
                                    if !function_name.is_empty() {
                                        if function_arguments.is_empty() {
                                            function_arguments = String::from("{}");
                                        }
                                        let arguments: Value =
                                        function_arguments.parse().with_context(|| {
                                            format!("Tool call '{function_name}' have non-JSON arguments '{function_arguments}'")
                                        })?;
                                        handler.tool_call(ToolCall::new(
                                            function_name.clone(),
                                            arguments,
                                            Some(function_id.clone()),
                                        ))?;
                                    }
                                    function_arguments.clear();
                                    function_name = name.into();
                                    function_id = id.into();
                                }
                            }
                        }
                        "contentBlockDelta" => {
                            if let Some(text) = data["delta"]["text"].as_str() {
                                handler.text(text)?;
                            } else if let Some(text) =
                                data["delta"]["reasoningContent"]["text"].as_str()
                            {
                                if reasoning_state == 0 {
                                    handler.text("<think>\n")?;
                                    reasoning_state = 1;
                                }
                                handler.text(text)?;
                            } else if let Some(input) = data["delta"]["toolUse"]["input"].as_str() {
                                function_arguments.push_str(input);
                            }
                        }
                        "contentBlockStop" => {
                            if reasoning_state == 1 {
                                handler.text("\n</think>\n\n")?;
                                reasoning_state = 0;
                            }
                            if !function_name.is_empty() {
                                if function_arguments.is_empty() {
                                    function_arguments = String::from("{}");
                                }
                                let arguments: Value = function_arguments.parse().with_context(|| {
                                    format!("Tool call '{function_name}' have non-JSON arguments '{function_arguments}'")
                                })?;
                                handler.tool_call(ToolCall::new(
                                    function_name.clone(),
                                    arguments,
                                    Some(function_id.clone()),
                                ))?;
                            }
                        }
                        _ => {}
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

async fn embeddings(builder: RequestBuilder) -> Result<EmbeddingsOutput> {
    let res = builder.send().await?;
    let status = res.status();
    let data: Value = res.json().await?;

    if !status.is_success() {
        catch_error(&data, status.as_u16())?;
    }

    let res_body: EmbeddingsResBody =
        serde_json::from_value(data).context("Invalid embeddings data")?;
    Ok(res_body.embeddings)
}

#[derive(Deserialize)]
struct EmbeddingsResBody {
    embeddings: Vec<Vec<f32>>,
}

fn build_chat_completions_body(data: ChatCompletionsData, model: &Model) -> Result<Value> {
    let ChatCompletionsData {
        mut messages,
        temperature,
        top_p,
        functions,
        stream: _,
    } = data;

    let system_message = extract_system_message(&mut messages);

    let mut network_image_urls = vec![];

    let messages_len = messages.len();
    let messages: Vec<Value> = messages
        .into_iter()
        .enumerate()
        .flat_map(|(i, message)| {
            let Message { role, content } = message;
            match content {
                MessageContent::Text(text) if role.is_assistant() && i != messages_len - 1 => {
                    vec![json!({ "role": role, "content": [ { "text": strip_think_tag(&text) } ] })]
                }
                MessageContent::Text(text) => vec![json!({
                    "role": role,
                    "content": [
                        {
                            "text": text,
                        }
                    ],
                })],
                MessageContent::Array(list) => {
                    let content: Vec<_> = list
                        .into_iter()
                        .map(|item| match item {
                            MessageContentPart::Text { text } => {
                                json!({"text": text})
                            }
                            MessageContentPart::ImageUrl {
                                image_url: ImageUrl { url },
                            } => {
                                if let Some((mime_type, data)) = url
                                    .strip_prefix("data:")
                                    .and_then(|v| v.split_once(";base64,"))
                                {
                                    json!({
                                        "image": {
                                            "format": mime_type.replace("image/", ""),
                                            "source": {
                                                "bytes": data,
                                            }
                                        }
                                    })
                                } else {
                                    network_image_urls.push(url.clone());
                                    json!({ "url": url })
                                }
                            }
                        })
                        .collect();
                    vec![json!({
                        "role": role,
                        "content": content,
                    })]
                }
                MessageContent::ToolCalls(MessageContentToolCalls {
                    tool_results, text, ..
                }) => {
                    let mut assistant_parts = vec![];
                    let mut user_parts = vec![];
                    if !text.is_empty() {
                        assistant_parts.push(json!({
                            "text": text,
                        }))
                    }
                    for tool_result in tool_results {
                        assistant_parts.push(json!({
                            "toolUse": {
                                "toolUseId": tool_result.call.id,
                                "name": tool_result.call.name,
                                "input": tool_result.call.arguments,
                            }
                        }));
                        user_parts.push(json!({
                            "toolResult": {
                                "toolUseId": tool_result.call.id,
                                "content": [
                                    {
                                        "json": tool_result.output,
                                    }
                                ]
                            }
                        }));
                    }
                    vec![
                        json!({
                            "role": "assistant",
                            "content": assistant_parts,
                        }),
                        json!({
                            "role": "user",
                            "content": user_parts,
                        }),
                    ]
                }
            }
        })
        .collect();

    if !network_image_urls.is_empty() {
        bail!(
            "The model does not support network images: {:?}",
            network_image_urls
        );
    }

    let mut body = json!({
        "inferenceConfig": {},
        "messages": messages,
    });
    if let Some(v) = system_message {
        body["system"] = json!([
            {
                "text": v,
            }
        ])
    }

    if let Some(v) = model.max_tokens_param() {
        body["inferenceConfig"]["maxTokens"] = v.into();
    }
    if let Some(v) = temperature {
        body["inferenceConfig"]["temperature"] = v.into();
    }
    if let Some(v) = top_p {
        body["inferenceConfig"]["topP"] = v.into();
    }
    if let Some(functions) = functions {
        let tools: Vec<_> = functions
            .iter()
            .map(|v| {
                json!({
                    "toolSpec": {
                        "name": v.name,
                        "description": v.description,
                        "inputSchema": {
                            "json": v.parameters,
                        },
                    }
                })
            })
            .collect();
        body["toolConfig"] = json!({
            "tools": tools,
        })
    }
    Ok(body)
}

fn extract_chat_completions(data: &Value) -> Result<ChatCompletionsOutput> {
    let mut text = String::new();
    let mut reasoning = None;
    let mut tool_calls = vec![];
    if let Some(array) = data["output"]["message"]["content"].as_array() {
        for item in array {
            if let Some(v) = item["text"].as_str() {
                if !text.is_empty() {
                    text.push_str("\n\n");
                }
                text.push_str(v);
            } else if let Some(reasoning_text) =
                item["reasoningContent"]["reasoningText"].as_object()
            {
                if let Some(text) = json_str_from_map(reasoning_text, "text") {
                    reasoning = Some(text.to_string());
                }
            } else if let Some(tool_use) = item["toolUse"].as_object() {
                if let (Some(id), Some(name), Some(input)) = (
                    json_str_from_map(tool_use, "toolUseId"),
                    json_str_from_map(tool_use, "name"),
                    tool_use.get("input"),
                ) {
                    tool_calls.push(ToolCall::new(
                        name.to_string(),
                        input.clone(),
                        Some(id.to_string()),
                    ))
                }
            }
        }
    }

    if let Some(reasoning) = reasoning {
        text = format!("<think>\n{reasoning}\n</think>\n\n{text}")
    }

    if text.is_empty() && tool_calls.is_empty() {
        bail!("Invalid response data: {data}");
    }

    let output = ChatCompletionsOutput {
        text,
        tool_calls,
        id: None,
        input_tokens: data["usage"]["inputTokens"].as_u64(),
        output_tokens: data["usage"]["outputTokens"].as_u64(),
    };
    Ok(output)
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

    debug!("Request {endpoint} {body}");

    let mut request_builder = client.request(method, endpoint).body(body);

    for (key, value) in &headers {
        request_builder = request_builder.header(key, value);
    }

    Ok(request_builder)
}

fn gen_signing_key(key: &str, date_stamp: &str, region: &str, service: &str) -> Vec<u8> {
    let k_date = hmac_sha256(format!("AWS4{}", key).as_bytes(), date_stamp);
    let k_region = hmac_sha256(&k_date, region);
    let k_service = hmac_sha256(&k_region, service);
    hmac_sha256(&k_service, "aws4_request")
}
