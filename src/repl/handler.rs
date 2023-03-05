use crate::client::ChatGptClient;
use crate::config::SharedConfig;
use crate::render::{self, MarkdownRender};
use crate::utils::dump;

use anyhow::Result;
use crossbeam::channel::{unbounded, Sender};
use crossbeam::sync::WaitGroup;
use std::cell::RefCell;
use std::fs::File;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::thread::spawn;

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
    ctrlc: Arc<AtomicBool>,
    render: Arc<MarkdownRender>,
}

pub struct ReplCmdHandlerState {
    reply: String,
    save_file: Option<File>,
}

impl ReplCmdHandler {
    pub fn init(client: ChatGptClient, config: SharedConfig) -> Result<Self> {
        let render = Arc::new(MarkdownRender::init()?);
        let save_file = config.as_ref().borrow().open_message_file()?;
        let ctrlc = Arc::new(AtomicBool::new(false));
        let state = RefCell::new(ReplCmdHandlerState {
            save_file,
            reply: String::new(),
        });
        Ok(Self {
            client,
            config,
            state,
            ctrlc,
            render,
        })
    }

    pub fn handle(&self, cmd: ReplCmd) -> Result<()> {
        match cmd {
            ReplCmd::Submit(input) => {
                if input.is_empty() {
                    self.state.borrow_mut().reply.clear();
                    return Ok(());
                }
                let prompt = self.config.borrow().get_prompt();
                let wg = WaitGroup::new();
                let highlight = self.config.borrow().highlight;
                let mut receiver = if highlight {
                    let (tx, rx) = unbounded();
                    let ctrlc = self.ctrlc.clone();
                    let wg = wg.clone();
                    let render = self.render.clone();
                    spawn(move || {
                        let _ = render::render_stream(rx, ctrlc, render);
                        drop(wg);
                    });
                    ReplyReceiver::new(Some(tx))
                } else {
                    ReplyReceiver::new(None)
                };
                self.client
                    .acquire_stream(&input, prompt, &mut receiver, self.ctrlc.clone())?;
                self.config.borrow().save_message(
                    self.state.borrow_mut().save_file.as_mut(),
                    &input,
                    &receiver.output,
                )?;
                wg.wait();
                self.state.borrow_mut().reply = receiver.output;
            }
            ReplCmd::SetRole(name) => {
                let output = self.config.borrow_mut().change_role(&name);
                dump(output.trim(), 2);
            }
            ReplCmd::ClearRole => {
                self.config.borrow_mut().role = None;
                dump("Done", 2);
            }
            ReplCmd::Prompt(prompt) => {
                let output = self.config.borrow_mut().create_temp_role(&prompt);
                dump(output.trim(), 2);
            }
            ReplCmd::Info => {
                let output = self.config.borrow().info()?;
                dump(output.trim(), 2);
            }
            ReplCmd::UpdateConfig(input) => {
                let output = self.config.borrow_mut().update(&input)?;
                dump(output.trim(), 2);
            }
        }
        Ok(())
    }

    pub fn get_reply(&self) -> String {
        self.state.borrow().reply.to_string()
    }

    pub fn get_ctrlc(&self) -> Arc<AtomicBool> {
        self.ctrlc.clone()
    }
}

pub struct ReplyReceiver {
    output: String,
    sender: Option<Sender<RenderStreamEvent>>,
}

impl ReplyReceiver {
    pub fn new(sender: Option<Sender<RenderStreamEvent>>) -> Self {
        Self {
            output: String::new(),
            sender,
        }
    }

    pub fn text(&mut self, text: &str) {
        match self.sender.as_ref() {
            Some(tx) => {
                let _ = tx.send(RenderStreamEvent::Text(text.to_string()));
            }
            None => {
                dump(text, 0);
            }
        }
        self.output.push_str(text);
    }

    pub fn done(&mut self) {
        match self.sender.as_ref() {
            Some(tx) => {
                let _ = tx.send(RenderStreamEvent::Done);
            }
            None => {
                dump("", 2);
            }
        }
    }
}

pub enum RenderStreamEvent {
    Text(String),
    Done,
}
