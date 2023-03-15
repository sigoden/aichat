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
    SetModel(String),
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
                let wg = WaitGroup::new();
                let ret = render_stream(
                    &input,
                    &self.client,
                    self.config.clone(),
                    true,
                    self.abort.clone(),
                    wg.clone(),
                );
                wg.wait();
                let buffer = ret?;
                self.config.read().save_message(&input, &buffer)?;
                self.config.write().save_conversation(&input, &buffer)?;
                *self.reply.borrow_mut() = buffer;
            }
            ReplCmd::SetModel(name) => {
                self.config.write().set_model(&name)?;
                print_now!("\n");
            }
            ReplCmd::SetRole(name) => {
                let output = self.config.write().change_role(&name)?;
                print_now!("{}\n\n", output.trim_end());
            }
            ReplCmd::ClearRole => {
                self.config.write().clear_role()?;
                print_now!("\n");
            }
            ReplCmd::Prompt(prompt) => {
                self.config.write().add_prompt(&prompt)?;
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
            ReplCmd::StartConversation => {
                self.config.write().start_conversation()?;
                print_now!("\n");
            }
            ReplCmd::EndConversatoin => {
                self.config.write().end_conversation();
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
        if self.buffer.is_empty() && text == "\n\n" {
            return Ok(());
        }
        self.buffer.push_str(text);
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
