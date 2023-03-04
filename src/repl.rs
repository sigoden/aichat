use crate::client::ChatGptClient;
use crate::config::{Config, Role};
use crate::editor;
use crate::render::{self, MarkdownRender};
use anyhow::{anyhow, Result};
use crossbeam::channel::{unbounded, Sender};
use crossbeam::sync::WaitGroup;
use reedline::{
    default_emacs_keybindings, ColumnarMenu, DefaultCompleter, DefaultPrompt, DefaultPromptSegment,
    Emacs, FileBackedHistory, KeyCode, KeyModifiers, Keybindings, Reedline, ReedlineEvent,
    ReedlineMenu, Signal,
};
use std::cell::RefCell;
use std::fs::File;
use std::io::{stdout, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::spawn;

const REPL_COMMANDS: [(&str, &str); 8] = [
    (".clear", "Clear the screen"),
    (".clear-history", "Clear the history"),
    (".clear-role", "Clear the role status"),
    (".editor", "Enter multiline editor"),
    (".exit", "Exit the REPL"),
    (".help", "Print this help message"),
    (".history", "Print the history"),
    (".role", "Specify the role that the AI will play"),
];

const MENU_NAME: &str = "completion_menu";

pub struct Repl {
    editor: Reedline,
    prompt: DefaultPrompt,
}

impl Repl {
    pub fn init(config: Arc<Config>) -> Result<Self> {
        let completer = Self::create_completer(config);
        let keybindings = Self::create_keybindings();
        let history = Self::create_history()?;
        let menu = Self::create_menu();
        let edit_mode = Box::new(Emacs::new(keybindings));
        let editor = Reedline::create()
            .with_completer(Box::new(completer))
            .with_history(history)
            .with_menu(menu)
            .with_edit_mode(edit_mode);
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
            let sig = self.editor.read_line(&self.prompt);
            match sig {
                Ok(Signal::Success(line)) => {
                    current_ctrlc = false;
                    match self.handle_line(handler.clone(), line) {
                        Ok(quit) => {
                            if quit {
                                break;
                            }
                        }
                        Err(err) => {
                            dump(format!("{err:?}"), 1);
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
                ".clear" => {
                    self.editor.clear_scrollback()?;
                }
                ".clear-history" => {
                    let history = Box::new(self.editor.history_mut());
                    history
                        .clear()
                        .map_err(|err| anyhow!("Failed to clear history, {err}"))?;
                    dump("", 1);
                }
                ".history" => {
                    self.editor.print_history()?;
                    dump("", 1);
                }
                ".role" => match args {
                    Some(name) => handler.handle(ReplCmd::SetRole(name.to_string()))?,
                    None => dump("Usage: .role <name>", 2),
                },
                ".clear-role" => {
                    handler.handle(ReplCmd::UnsetRole)?;
                    dump("", 1);
                }
                ".editor" => {
                    dump(
                        "// Entering editor mode (Ctrl+D to finish, Ctrl+C to cancel)",
                        1,
                    );
                    let content = editor::edit()?;
                    dump("", 1);
                    handler.handle(ReplCmd::Input(content))?;
                }
                _ => dump_unknown_command(),
            }
        } else {
            handler.handle(ReplCmd::Input(line))?;
        }

        Ok(false)
    }

    fn create_prompt() -> DefaultPrompt {
        DefaultPrompt::new(DefaultPromptSegment::Empty, DefaultPromptSegment::Empty)
    }

    fn create_completer(config: Arc<Config>) -> DefaultCompleter {
        let mut commands: Vec<String> = REPL_COMMANDS
            .into_iter()
            .map(|(v, _)| v.to_string())
            .collect();
        commands.extend(config.roles.iter().map(|v| format!(".role {}", v.name)));
        let mut completer = DefaultCompleter::with_inclusions(&['.', '-']).set_min_word_len(2);
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
        keybindings
    }

    fn create_menu() -> ReedlineMenu {
        let completion_menu = ColumnarMenu::default().with_name(MENU_NAME);
        ReedlineMenu::EngineCompleter(Box::new(completion_menu))
    }

    fn create_history() -> Result<Box<FileBackedHistory>> {
        Ok(Box::new(
            FileBackedHistory::with_file(1000, Config::history_file()?)
                .map_err(|err| anyhow!("Failed to setup history file, {err}"))?,
        ))
    }
}

pub struct ReplCmdHandler {
    client: ChatGptClient,
    config: Arc<Config>,
    state: RefCell<ReplCmdHandlerState>,
    ctrlc: Arc<AtomicBool>,
    render: Option<Arc<MarkdownRender>>,
}

struct ReplCmdHandlerState {
    prompt: String,
    output: String,
    save_file: Option<File>,
}

impl ReplCmdHandler {
    pub fn init(client: ChatGptClient, config: Arc<Config>, role: Option<Role>) -> Result<Self> {
        let render = if config.no_highlight {
            None
        } else {
            Some(Arc::new(MarkdownRender::init()?))
        };
        let prompt = role.map(|v| v.prompt).unwrap_or_default();
        let save_file = config.open_message_file()?;
        let ctrlc = Arc::new(AtomicBool::new(false));
        let state = RefCell::new(ReplCmdHandlerState {
            prompt,
            save_file,
            output: String::new(),
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
            ReplCmd::Input(input) => {
                if input.is_empty() {
                    self.state.borrow_mut().output.clear();
                    return Ok(());
                }
                let prompt = self.state.borrow().prompt.to_string();
                let prompt = if prompt.is_empty() {
                    None
                } else {
                    Some(prompt)
                };
                let wg = WaitGroup::new();
                let mut receiver = if let Some(markdown_render) = self.render.clone() {
                    let (tx, rx) = unbounded();
                    let ctrlc = self.ctrlc.clone();
                    let wg = wg.clone();
                    spawn(move || {
                        let _ = render::render_stream(rx, ctrlc, markdown_render);
                        drop(wg);
                    });
                    ReplyReceiver::new(Some(tx))
                } else {
                    ReplyReceiver::new(None)
                };
                self.client
                    .acquire_stream(&input, prompt, &mut receiver, self.ctrlc.clone())?;
                Config::save_message(
                    self.state.borrow_mut().save_file.as_mut(),
                    &input,
                    &receiver.output,
                );
                wg.wait();
                self.state.borrow_mut().output = receiver.output;
            }
            ReplCmd::SetRole(name) => match self.config.find_role(&name) {
                Some(v) => {
                    self.state.borrow_mut().prompt = v.prompt;
                    dump("", 1);
                }
                None => {
                    dump("Unknown role", 2);
                }
            },
            ReplCmd::UnsetRole => {
                self.state.borrow_mut().prompt = String::new();
            }
        }
        Ok(())
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

pub fn dump<T: ToString>(text: T, newlines: usize) {
    print!("{}{}", text.to_string(), "\n".repeat(newlines));
    let _ = stdout().flush();
}

enum ReplCmd {
    UnsetRole,
    Input(String),
    SetRole(String),
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
