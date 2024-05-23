use super::catch_error;

use anyhow::{anyhow, bail, Result};
use futures_util::{Stream, StreamExt};
use reqwest::RequestBuilder;
use reqwest_eventsource::{Error as EventSourceError, Event, RequestBuilderExt};
use serde_json::Value;

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

pub async fn json_stream<S, F, E>(mut stream: S, mut handle: F) -> Result<()>
where
    S: Stream<Item = Result<bytes::Bytes, E>> + Unpin,
    F: FnMut(&str) -> Result<()>,
    E: std::error::Error,
{
    let mut parser = JsonStreamParser::default();
    let mut unparsed_bytes = vec![];
    while let Some(chunk_bytes) = stream.next().await {
        let chunk_bytes =
            chunk_bytes.map_err(|err| anyhow!("Failed to read json stream, {err}"))?;
        unparsed_bytes.extend(chunk_bytes);
        match std::str::from_utf8(&unparsed_bytes) {
            Ok(text) => {
                parser.process(text, &mut handle)?;
                unparsed_bytes.clear();
            }
            Err(_) => {
                continue;
            }
        }
    }
    if !unparsed_bytes.is_empty() {
        let text = std::str::from_utf8(&unparsed_bytes)?;
        parser.process(text, &mut handle)?;
    }

    Ok(())
}

#[derive(Debug, Default)]
struct JsonStreamParser {
    buffer: Vec<char>,
    cursor: usize,
    start: Option<usize>,
    balances: Vec<char>,
    quoting: bool,
    escape: bool,
}

impl JsonStreamParser {
    fn process<F>(&mut self, text: &str, handle: &mut F) -> Result<()>
    where
        F: FnMut(&str) -> Result<()>,
    {
        self.buffer.extend(text.chars());

        for i in self.cursor..self.buffer.len() {
            let ch = self.buffer[i];
            if self.quoting {
                if ch == '\\' {
                    self.escape = !self.escape;
                } else {
                    if !self.escape && ch == '"' {
                        self.quoting = false;
                    }
                    self.escape = false;
                }
                continue;
            }
            match ch {
                '"' => {
                    self.quoting = true;
                    self.escape = false;
                }
                '{' => {
                    if self.balances.is_empty() {
                        self.start = Some(i);
                    }
                    self.balances.push(ch);
                }
                '[' => {
                    if self.start.is_some() {
                        self.balances.push(ch);
                    }
                }
                '}' => {
                    self.balances.pop();
                    if self.balances.is_empty() {
                        if let Some(start) = self.start.take() {
                            let value: String = self.buffer[start..=i].iter().collect();
                            handle(&value)?;
                        }
                    }
                }
                ']' => {
                    self.balances.pop();
                }
                _ => {}
            }
        }
        self.cursor = self.buffer.len();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use bytes::Bytes;
    use futures_util::stream;

    macro_rules! assert_json_stream {
        ($input:expr, $output:expr) => {
            let stream = stream::iter(vec![Ok::<_, std::convert::Infallible>(Bytes::from($input))]);
            let mut output = vec![];
            let ret = json_stream(stream, |data| {
                output.push(data.to_string());
                Ok(())
            })
            .await;
            assert!(ret.is_ok());
            assert_eq!($output.replace("\r\n", "\n"), output.join("\n"))
        };
    }

    #[tokio::test]
    async fn test_json_stream_ndjson() {
        let data = r#"{"key": "value"}
{"key": "value2"}
{"key": "value3"}"#;
        assert_json_stream!(data, data);
    }

    #[tokio::test]
    async fn test_json_stream_array() {
        let input = r#"[
{"key": "value"},
{"key": "value2"},
{"key": "value3"},"#;
        let output = r#"{"key": "value"}
{"key": "value2"}
{"key": "value3"}"#;
        assert_json_stream!(input, output);
    }
}
