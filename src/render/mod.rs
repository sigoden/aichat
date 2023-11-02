mod cmd;
mod markdown;
mod repl;

use self::cmd::cmd_render_stream;
pub use self::markdown::{MarkdownRender, RenderOptions};
use self::repl::repl_render_stream;

use crate::client::Client;
use crate::config::GlobalConfig;
use crate::utils::AbortSignal;

use anyhow::{Context, Result};
use crossbeam::channel::{unbounded, Sender};
use crossbeam::sync::WaitGroup;
use nu_ansi_term::{Color, Style};
use std::thread::spawn;

pub fn render_stream(
    input: &str,
    client: &dyn Client,
    config: &GlobalConfig,
    repl: bool,
    abort: AbortSignal,
    wg: WaitGroup,
) -> Result<String> {
    let render_options = config.read().get_render_options()?;
    let mut stream_handler = {
        let (tx, rx) = unbounded();
        let abort_clone = abort.clone();
        let highlight = config.read().highlight;
        spawn(move || {
            let run = move || {
                if repl {
                    let mut render = MarkdownRender::init(render_options)?;
                    repl_render_stream(&rx, &mut render, &abort)
                } else {
                    let mut render = MarkdownRender::init(render_options)?;
                    cmd_render_stream(&rx, &mut render, &abort)
                }
            };
            if let Err(err) = run() {
                render_error(err, highlight);
            }
            drop(wg);
        });
        ReplyHandler::new(tx, abort_clone)
    };
    client.send_message_streaming(input, &mut stream_handler)?;
    let buffer = stream_handler.get_buffer();
    Ok(buffer.to_string())
}

pub fn render_error(err: anyhow::Error, highlight: bool) {
    let err = format!("{err:?}");
    if highlight {
        let style = Style::new().fg(Color::Red);
        println!("{}", style.paint(err.trim()));
    } else {
        println!("{}", err.trim());
    }
}

pub struct ReplyHandler {
    sender: Sender<ReplyEvent>,
    buffer: String,
    abort: AbortSignal,
}

impl ReplyHandler {
    pub fn new(sender: Sender<ReplyEvent>, abort: AbortSignal) -> Self {
        Self {
            sender,
            abort,
            buffer: String::new(),
        }
    }

    pub fn text(&mut self, text: &str) -> Result<()> {
        debug!("ReplyText: {}", text);
        if self.buffer.is_empty() && text == "\n\n" {
            return Ok(());
        }
        self.buffer.push_str(text);
        let ret = self
            .sender
            .send(ReplyEvent::Text(text.to_string()))
            .with_context(|| "Failed to send ReplyEvent:Text");
        self.safe_ret(ret)?;
        Ok(())
    }

    pub fn done(&mut self) -> Result<()> {
        debug!("ReplyDone");
        let ret = self
            .sender
            .send(ReplyEvent::Done)
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

pub enum ReplyEvent {
    Text(String),
    Done,
}
