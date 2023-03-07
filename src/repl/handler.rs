use crate::client::ChatGptClient;
use crate::config::SharedConfig;
use crate::render::render_stream;
use crate::utils::dump;

use anyhow::Result;
use crossbeam::channel::Sender;
use crossbeam::sync::WaitGroup;
use std::cell::RefCell;
use std::fs::File;

use super::abort::SharedAbortSignal;

pub enum ReplCmd {
    Submit(String),
    SetRole(String),
    UpdateConfig(String),
    Prompt(String),
    ClearRole,
    Info,
}

pub struct ReplCmdHandler {
    client: ChatGptClient,
    config: SharedConfig,
    state: RefCell<ReplCmdHandlerState>,
    abort: SharedAbortSignal,
}

pub struct ReplCmdHandlerState {
    reply: String,
    save_file: Option<File>,
}

impl ReplCmdHandler {
    pub fn init(
        client: ChatGptClient,
        config: SharedConfig,
        abort: SharedAbortSignal,
    ) -> Result<Self> {
        let save_file = config.as_ref().borrow().open_message_file()?;
        let state = RefCell::new(ReplCmdHandlerState {
            save_file,
            reply: String::new(),
        });
        Ok(Self {
            client,
            config,
            state,
            abort,
        })
    }

    pub fn handle(&self, cmd: ReplCmd) -> Result<()> {
        match cmd {
            ReplCmd::Submit(input) => {
                if input.is_empty() {
                    self.state.borrow_mut().reply.clear();
                    return Ok(());
                }
                let highlight = self.config.borrow().highlight;
                let prompt = self.config.borrow().get_prompt();
                let wg = WaitGroup::new();
                let ret = render_stream(
                    &input,
                    prompt,
                    &self.client,
                    highlight,
                    true,
                    self.abort.clone(),
                    wg.clone(),
                );
                wg.wait();
                let buffer = ret?;
                self.config.borrow().save_message(
                    self.state.borrow_mut().save_file.as_mut(),
                    &input,
                    &buffer,
                )?;
                self.state.borrow_mut().reply = buffer;
            }
            ReplCmd::SetRole(name) => {
                let output = self.config.borrow_mut().change_role(&name);
                dump(output, 1);
            }
            ReplCmd::ClearRole => {
                self.config.borrow_mut().role = None;
                dump("", 1);
            }
            ReplCmd::Prompt(prompt) => {
                self.config.borrow_mut().create_temp_role(&prompt);
                dump("", 1);
            }
            ReplCmd::Info => {
                let output = self.config.borrow().info()?;
                dump(output, 1);
            }
            ReplCmd::UpdateConfig(input) => {
                let output = self.config.borrow_mut().update(&input)?;
                dump(output, 1);
            }
        }
        Ok(())
    }
}

pub struct ReplyStreamHandler {
    sender: Option<Sender<ReplyStreamEvent>>,
    buffer: String,
    abort: SharedAbortSignal,
}

impl ReplyStreamHandler {
    pub fn new(sender: Option<Sender<ReplyStreamEvent>>, abort: SharedAbortSignal) -> Self {
        Self {
            sender,
            abort,
            buffer: String::new(),
        }
    }

    pub fn text(&mut self, text: &str) {
        match self.sender.as_ref() {
            Some(tx) => {
                let _ = tx.send(ReplyStreamEvent::Text(text.to_string()));
            }
            None => {
                dump(text, 0);
            }
        }
        self.buffer.push_str(text);
    }

    pub fn done(&mut self) {
        match self.sender.as_ref() {
            Some(tx) => {
                let _ = tx.send(ReplyStreamEvent::Done);
            }
            None => {
                dump("", 2);
            }
        }
    }

    pub fn get_buffer(&self) -> &str {
        &self.buffer
    }

    pub fn get_abort(&self) -> SharedAbortSignal {
        self.abort.clone()
    }
}

pub enum ReplyStreamEvent {
    Text(String),
    Done,
}
