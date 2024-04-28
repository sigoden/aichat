use crate::utils::AbortSignal;

use anyhow::{Context, Result};
use tokio::sync::mpsc::UnboundedSender;

pub struct SseHandler {
    sender: UnboundedSender<SseEvent>,
    buffer: String,
    abort: AbortSignal,
}

impl SseHandler {
    pub fn new(sender: UnboundedSender<SseEvent>, abort: AbortSignal) -> Self {
        Self {
            sender,
            abort,
            buffer: String::new(),
        }
    }

    pub fn text(&mut self, text: &str) -> Result<()> {
        // debug!("ReplyText: {}", text);
        if text.is_empty() {
            return Ok(());
        }
        self.buffer.push_str(text);
        let ret = self
            .sender
            .send(SseEvent::Text(text.to_string()))
            .with_context(|| "Failed to send ReplyEvent:Text");
        self.safe_ret(ret)?;
        Ok(())
    }

    pub fn done(&mut self) -> Result<()> {
        // debug!("ReplyDone");
        let ret = self
            .sender
            .send(SseEvent::Done)
            .with_context(|| "Failed to send ReplyEvent::Done");
        self.safe_ret(ret)?;
        Ok(())
    }

    pub fn get_buffer(&self) -> &str {
        &self.buffer
    }

    pub fn get_abort(&self) -> AbortSignal {
        self.abort.clone()
    }

    fn safe_ret(&self, ret: Result<()>) -> Result<()> {
        if ret.is_err() && self.abort.aborted() {
            return Ok(());
        }
        ret
    }
}

#[derive(Debug)]
pub enum SseEvent {
    Text(String),
    Done,
}
