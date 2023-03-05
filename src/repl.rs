use crate::client::ChatGptClient;
use crate::config::{Config, SharedConfig};
use crate::render::{self, MarkdownRender};
use crate::term;
use crate::utils::{copy, dump};
use anyhow::{Context, Result};
use crossbeam::channel::{unbounded, Sender};
use crossbeam::sync::WaitGroup;
use reedline::{
    default_emacs_keybindings, ColumnarMenu, DefaultCompleter, DefaultPrompt, DefaultPromptSegment,
    Emacs, FileBackedHistory, KeyCode, KeyModifiers, Keybindings, Reedline, ReedlineEvent,
    ReedlineMenu, Signal, ValidationResult, Validator,
};
use std::cell::RefCell;
use std::fs::File;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::spawn;

const REPL_COMMANDS: [(&str, &str); 12] = [
    (".role", "Specifies the role the AI will play"),
    (".clear role", "Clear the currently selected role"),
    (".prompt", "Add prompt, aka create a temporary role"),
    (".history", "Print the history"),
    (".clear history", "Clear the history"),
    (".multiline", "Enter multiline editor mode"),
    (".copy", "Copy last reply message"),
    (".info", "Print the information"),
    (".set", "Modify the configuration temporarily"),
    (".help", "Print this help message"),
    (".exit", "Exit the REPL"),
    (".clear screen", "Clear the screen"),
];

const MENU_NAME: &str = "completion_menu";

pub struct Repl {
    editor: Reedline,
    prompt: DefaultPrompt,
}

impl Repl {
    pub fn init(config: SharedConfig) -> Result<Self> {
        let completer = Self::create_completer(config);
        let keybindings = Self::create_keybindings();
        let history = Self::create_history()?;
        let menu = Self::create_menu();
        let edit_mode = Box::new(Emacs::new(keybindings));
        let editor = Reedline::create()
            .with_completer(Box::new(completer))
            .with_history(history)
            .with_menu(menu)
            .with_edit_mode(edit_mode)
            .with_quick_completions(true)
            .with_partial_completions(true)
            .with_validator(Box::new(ReplValidator {
                multiline_cmds: [".multiline", ".prompt"].to_vec(),
            }))
            .with_ansi_colors(true);
        let prompt = Self::create_prompt();
        Ok(Self { editor, prompt })
    }

    pub fn run(&mut self, handler: ReplCmdHandler) -> Result<()> {
        dump(
            format!("Welcome to aichat {}", env!("CARGO_PKG_VERSION")),
            1,
        );
        dump("Type \".help\" for more information.", 1);
        let mut current_ctrlc = false;
        let handler = Arc::new(handler);
        loop {
            if handler.ctrlc.load(Ordering::SeqCst) {
                handler.ctrlc.store(false, Ordering::SeqCst);
                current_ctrlc = true
            }
            match self.editor.read_line(&self.prompt) {
                Ok(Signal::Success(line)) => {
                    current_ctrlc = false;
                    match self.handle_line(handler.clone(), line) {
                        Ok(quit) => {
                            if quit {
                                break;
                            }
                        }
                        Err(err) => {
                            let err = format!("{err:?}");
                            dump(err.trim(), 2);
                        }
                    }
                }
                Ok(Signal::CtrlC) => {
                    if !current_ctrlc {
                        current_ctrlc = true;
                        dump("(To exit, press Ctrl+C again or Ctrl+D or type .exit)", 2);
                    } else {
                        break;
                    }
                }
                Ok(Signal::CtrlD) => {
                    break;
                }
                _ => {}
            }
        }
        Ok(())
    }

    fn handle_line(&mut self, handler: Arc<ReplCmdHandler>, line: String) -> Result<bool> {
        if line.starts_with('.') {
            let (cmd, args) = match line.split_once(' ') {
                Some((head, tail)) => (head, Some(tail.trim())),
                None => (line.as_str(), None),
            };
            match cmd {
                ".exit" => {
                    return Ok(true);
                }
                ".help" => {
                    dump_repl_help();
                }
                ".clear" => match args {
                    Some("screen") => term::clear_screen(0)?,
                    Some("history") => {
                        let history = Box::new(self.editor.history_mut());
                        history.clear().with_context(|| "Failed to clear history")?;
                        dump("", 1);
                    }
                    Some("role") => handler.handle(ReplCmd::ClearRole)?,
                    _ => dump_unknown_command(),
                },
                ".history" => {
                    self.editor.print_history()?;
                    dump("", 1);
                }
                ".role" => match args {
                    Some(name) => handler.handle(ReplCmd::SetRole(name.to_string()))?,
                    None => dump("Usage: .role <name>", 2),
                },
                ".info" => {
                    handler.handle(ReplCmd::Info)?;
                }
                ".multiline" => {
                    let mut text = args.unwrap_or_default().to_string();
                    if text.is_empty() {
                        dump("Usage: .multiline { <your multiline content> }", 2);
                    } else {
                        if text.starts_with('{') && text.ends_with('}') {
                            text = text[1..text.len() - 1].to_string()
                        }
                        handler.handle(ReplCmd::Submit(text))?;
                    }
                }
                ".copy" => {
                    let reply = handler.get_reply();
                    if reply.is_empty() {
                        dump("No reply messages that can be copied", 1)
                    } else {
                        copy(&reply)?;
                        dump("Copied", 1);
                    }
                }
                ".set" => {
                    handler.handle(ReplCmd::UpdateConfig(args.unwrap_or_default().to_string()))?
                }
                ".prompt" => {
                    let mut text = args.unwrap_or_default().to_string();
                    if text.is_empty() {
                        dump("Usage: .prompt { <your multiline content> }.", 2);
                    } else {
                        if text.starts_with('{') && text.ends_with('}') {
                            text = text[1..text.len() - 1].to_string()
                        }
                        handler.handle(ReplCmd::Prompt(text))?;
                    }
                }
                _ => dump_unknown_command(),
            }
        } else {
            handler.handle(ReplCmd::Submit(line))?;
        }

        Ok(false)
    }

    fn create_prompt() -> DefaultPrompt {
        DefaultPrompt::new(DefaultPromptSegment::Empty, DefaultPromptSegment::Empty)
    }

    fn create_completer(config: SharedConfig) -> DefaultCompleter {
        let mut commands: Vec<String> = REPL_COMMANDS
            .into_iter()
            .map(|(v, _)| v.to_string())
            .collect();
        commands.extend(
            config
                .as_ref()
                .borrow()
                .roles
                .iter()
                .map(|v| format!(".role {}", v.name)),
        );
        commands.extend(Config::UPDATE_KEYS.map(|v| format!(".set {v}")));
        let mut completer = DefaultCompleter::with_inclusions(&['.', '-', '_']).set_min_word_len(2);
        completer.insert(commands.clone());
        completer
    }

    fn create_keybindings() -> Keybindings {
        let mut keybindings = default_emacs_keybindings();
        keybindings.add_binding(
            KeyModifiers::NONE,
            KeyCode::Tab,
            ReedlineEvent::UntilFound(vec![
                ReedlineEvent::Menu(MENU_NAME.to_string()),
                ReedlineEvent::MenuNext,
            ]),
        );
        keybindings.add_binding(
            KeyModifiers::CONTROL,
            KeyCode::Char('l'),
            ReedlineEvent::ExecuteHostCommand(".clear screen".into()),
        );
        keybindings
    }

    fn create_menu() -> ReedlineMenu {
        let completion_menu = ColumnarMenu::default().with_name(MENU_NAME);
        ReedlineMenu::EngineCompleter(Box::new(completion_menu))
    }

    fn create_history() -> Result<Box<FileBackedHistory>> {
        Ok(Box::new(
            FileBackedHistory::with_file(1000, Config::history_file()?)
                .with_context(|| "Failed to setup history file")?,
        ))
    }
}
pub struct ReplValidator {
    multiline_cmds: Vec<&'static str>,
}

impl Validator for ReplValidator {
    fn validate(&self, line: &str) -> ValidationResult {
        if line.split('"').count() % 2 == 0 || incomplete_brackets(line, &self.multiline_cmds) {
            ValidationResult::Incomplete
        } else {
            ValidationResult::Complete
        }
    }
}

fn incomplete_brackets(line: &str, multiline_cmds: &[&str]) -> bool {
    let mut balance: Vec<char> = Vec::new();
    let line = line.trim_start();
    if !multiline_cmds.iter().any(|v| line.starts_with(v)) {
        return false;
    }

    for c in line.chars() {
        if c == '{' {
            balance.push('}');
        } else if c == '}' {
            if let Some(last) = balance.last() {
                if last == &c {
                    balance.pop();
                }
            }
        }
    }

    !balance.is_empty()
}

pub struct ReplCmdHandler {
    client: ChatGptClient,
    config: SharedConfig,
    state: RefCell<ReplCmdHandlerState>,
    ctrlc: Arc<AtomicBool>,
    render: Arc<MarkdownRender>,
}

struct ReplCmdHandlerState {
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
    fn handle(&self, cmd: ReplCmd) -> Result<()> {
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
                );
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

    fn get_reply(&self) -> String {
        self.state.borrow().reply.to_string()
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

enum ReplCmd {
    Submit(String),
    SetRole(String),
    UpdateConfig(String),
    Prompt(String),
    ClearRole,
    Info,
}

fn dump_repl_help() {
    let head = REPL_COMMANDS
        .iter()
        .map(|(name, desc)| format!("{name:<15} {desc}"))
        .collect::<Vec<String>>()
        .join("\n");
    dump(
        format!("{head}\n\nPress Ctrl+C to abort session, Ctrl+D to exit the REPL"),
        2,
    );
}

fn dump_unknown_command() {
    dump("Unknown command. Type \".help\" for more information.", 2);
}
