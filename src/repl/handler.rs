use crate::client::ChatGptClient;
use crate::config::SharedConfig;
use crate::print_now;
use crate::render::render_stream;

use super::abort::SharedAbortSignal;

use anyhow::{Context, Result};
use crossbeam::channel::Sender;
use crossbeam::sync::WaitGroup;
use std::cell::RefCell;

pub enum ReplCmd {
    Submit(String),
    SetRole(String),
    UpdateConfig(String),
    Prompt(String),
    ClearRole,
    ViewInfo,
    StartConversation,
    EndConversatoin,
}

pub struct ReplCmdHandler {
    client: ChatGptClient,
    config: SharedConfig,
    reply: RefCell<String>,
    abort: SharedAbortSignal,
}

impl ReplCmdHandler {
    pub fn init(
        client: ChatGptClient,
        config: SharedConfig,
        abort: SharedAbortSignal,
    ) -> Result<Self> {
        let reply = RefCell::new(String::new());
        Ok(Self {
            client,
            config,
            reply,
            abort,
        })
    }

    pub fn handle(&self, cmd: ReplCmd) -> Result<()> {
        match cmd {
            ReplCmd::Submit(input) => {
                if input.is_empty() {
                    self.reply.borrow_mut().clear();
                    return Ok(());
                }
                let highlight = self.config.lock().highlight;
                let wg = WaitGroup::new();
                let ret = render_stream(
                    &input,
                    &self.client,
                    highlight,
                    true,
                    self.abort.clone(),
                    wg.clone(),
                );
                wg.wait();
                let buffer = ret?;
                self.config.lock().save_message(&input, &buffer)?;
                self.config.lock().save_conversation(&input, &buffer)?;
                *self.reply.borrow_mut() = buffer;
            }
            ReplCmd::SetRole(name) => {
                let output = self.config.lock().change_role(&name)?;
                print_now!("{}\n\n", output.trim_end());
            }
            ReplCmd::ClearRole => {
                self.config.lock().role = None;
                print_now!("\n");
            }
            ReplCmd::Prompt(prompt) => {
                self.config.lock().create_temp_role(&prompt)?;
                print_now!("\n");
            }
            ReplCmd::ViewInfo => {
                let output = self.config.lock().info()?;
                print_now!("{}\n\n", output.trim_end());
            }
            ReplCmd::UpdateConfig(input) => {
                self.config.lock().update(&input)?;
                print_now!("\n");
            }
            ReplCmd::StartConversation => {
                self.config.lock().start_conversation()?;
                print_now!("\n");
            }
            ReplCmd::EndConversatoin => {
                self.config.lock().end_conversation();
                print_now!("\n");
            }
        }
        Ok(())
    }
}

pub struct ReplyStreamHandler {
    sender: Option<Sender<ReplyStreamEvent>>,
    buffer: String,
    abort: SharedAbortSignal,
    repl: bool,
}

impl ReplyStreamHandler {
    pub fn new(
        sender: Option<Sender<ReplyStreamEvent>>,
        repl: bool,
        abort: SharedAbortSignal,
    ) -> Self {
        Self {
            sender,
            abort,
            buffer: String::new(),
            repl,
        }
    }

    pub fn text(&mut self, text: &str) -> Result<()> {
        match self.sender.as_ref() {
            Some(tx) => {
                let ret = tx
                    .send(ReplyStreamEvent::Text(text.to_string()))
                    .with_context(|| "Failed to send StreamEvent:Text");
                self.safe_ret(ret)?;
            }
            None => {
                print_now!("{}", text);
            }
        }
        self.buffer.push_str(text);
        Ok(())
    }

    pub fn done(&mut self) -> Result<()> {
        match self.sender.as_ref() {
            Some(tx) => {
                let ret = tx
                    .send(ReplyStreamEvent::Done)
                    .with_context(|| "Failed to send StreamEvent:Done");
                self.safe_ret(ret)?;
            }
            None => {
                if !self.buffer.ends_with('\n') {
                    print_now!("\n")
                }
                if self.repl {
                    print_now!("\n");
                    if cfg!(macos) {
                        print_now!("\n")
                    }
                }
            }
        }
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
