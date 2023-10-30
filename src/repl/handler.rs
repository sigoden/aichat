use crate::client::init_client;
use crate::config::SharedConfig;
use crate::print_now;
use crate::render::render_stream;
use std::fs;
use std::io::Read;

use super::abort::SharedAbortSignal;

use anyhow::{bail, Context, Result};
use arboard::Clipboard;
use crossbeam::channel::Sender;
use crossbeam::sync::WaitGroup;
use std::cell::RefCell;

pub enum ReplCmd {
    Submit(String),
    SetModel(String),
    SetRole(String),
    UpdateConfig(String),
    ClearRole,
    ViewInfo,
    StartSession(Option<String>),
    EndSession,
    Copy,
    ReadFile(String),
}

pub struct ReplCmdHandler {
    config: SharedConfig,
    abort: SharedAbortSignal,
    clipboard: std::result::Result<RefCell<Clipboard>, arboard::Error>,
}

impl ReplCmdHandler {
    pub fn init(config: SharedConfig, abort: SharedAbortSignal) -> Result<Self> {
        let clipboard = Clipboard::new().map(RefCell::new);
        Ok(Self {
            config,
            abort,
            clipboard,
        })
    }

    pub fn handle(&self, cmd: ReplCmd) -> Result<()> {
        match cmd {
            ReplCmd::Submit(input) => {
                if input.is_empty() {
                    return Ok(());
                }
                self.config.read().maybe_print_send_tokens(&input);
                let wg = WaitGroup::new();
                let client = init_client(self.config.clone())?;
                let ret = render_stream(
                    &input,
                    client.as_ref(),
                    &self.config,
                    true,
                    self.abort.clone(),
                    wg.clone(),
                );
                wg.wait();
                let buffer = ret?;
                self.config.write().save_message(&input, &buffer)?;
                if self.config.read().auto_copy {
                    let _ = self.copy(&buffer);
                }
            }
            ReplCmd::SetModel(name) => {
                self.config.write().set_model(&name)?;
                print_now!("\n");
            }
            ReplCmd::SetRole(name) => {
                let output = self.config.write().set_role(&name)?;
                print_now!("{}\n\n", output.trim_end());
            }
            ReplCmd::ClearRole => {
                self.config.write().clear_role()?;
                print_now!("\n");
            }
            ReplCmd::ViewInfo => {
                let output = self.config.read().info()?;
                print_now!("{}\n\n", output.trim_end());
            }
            ReplCmd::UpdateConfig(input) => {
                self.config.write().update(&input)?;
                print_now!("\n");
            }
            ReplCmd::StartSession(name) => {
                self.config.write().start_session(&name)?;
                print_now!("\n");
            }
            ReplCmd::EndSession => {
                self.config.write().end_session()?;
                print_now!("\n");
            }
            ReplCmd::Copy => {
                let reply = self
                    .config
                    .read()
                    .last_message
                    .as_ref()
                    .map(|v| v.1.clone())
                    .unwrap_or_default();
                self.copy(&reply)
                    .with_context(|| "Failed to copy the last output")?;
                print_now!("\n");
            }
            ReplCmd::ReadFile(file) => {
                let mut contents = String::new();
                let mut file = fs::File::open(file).with_context(|| "Unable to open file")?;
                file.read_to_string(&mut contents)
                    .with_context(|| "Unable to read file")?;
                self.handle(ReplCmd::Submit(contents))?;
            }
        }
        Ok(())
    }

    fn copy(&self, text: &str) -> Result<()> {
        match self.clipboard.as_ref() {
            Err(err) => bail!("{}", err),
            Ok(clip) => {
                clip.borrow_mut().set_text(text)?;
                Ok(())
            }
        }
    }
}

pub struct ReplyStreamHandler {
    sender: Sender<ReplyStreamEvent>,
    buffer: String,
    abort: SharedAbortSignal,
}

impl ReplyStreamHandler {
    pub fn new(sender: Sender<ReplyStreamEvent>, abort: SharedAbortSignal) -> Self {
        Self {
            sender,
            abort,
            buffer: String::new(),
        }
    }

    pub fn text(&mut self, text: &str) -> Result<()> {
        if self.buffer.is_empty() && text == "\n\n" {
            return Ok(());
        }
        self.buffer.push_str(text);
        let ret = self
            .sender
            .send(ReplyStreamEvent::Text(text.to_string()))
            .with_context(|| "Failed to send StreamEvent:Text");
        self.safe_ret(ret)?;
        Ok(())
    }

    pub fn done(&mut self) -> Result<()> {
        let ret = self
            .sender
            .send(ReplyStreamEvent::Done)
            .with_context(|| "Failed to send StreamEvent:Done");
        self.safe_ret(ret)?;
        Ok(())
    }

    pub fn get_buffer(&self) -> &str {
        &self.buffer
    }

    pub fn get_abort(&self) -> SharedAbortSignal {
        self.abort.clone()
    }

    fn safe_ret(&self, ret: Result<()>) -> Result<()> {
        if ret.is_err() && self.abort.aborted() {
            return Ok(());
        }
        ret
    }
}

pub enum ReplyStreamEvent {
    Text(String),
    Done,
}
