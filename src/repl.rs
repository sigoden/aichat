use crate::client::ChatGptClient;
use crate::config::{Config, Role};
use anyhow::{anyhow, Result};
use inquire::Editor;
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

const REPL_COMMANDS: [(&str, &str); 8] = [
    (".clear", "Clear the screen"),
    (".clear-history", "Clear the history"),
    (".clear-role", "Clear the role status"),
    (".exit", "Exit the REPL"),
    (".help", "Print this help message"),
    (".history", "Print the history"),
    (".role", "Specify the role that the AI will play"),
    (".view", "Use an external editor to view the AI reply"),
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
                Err(err) => {
                    dump(format!("{err:?}"), 1);
                    break;
                }
            }
        }
        // tx.send(ReplCmd::Quit).unwrap();
        Ok(())
    }

    fn handle_line(&mut self, handler: Arc<ReplCmdHandler>, line: String) -> Result<bool> {
        if line.starts_with('.') {
            let (cmd, args) = match line.split_once(' ') {
                Some((head, tail)) => (head, Some(tail.trim())),
                None => (line.as_str(), None),
            };
            match cmd {
                ".view" => handler.handle(ReplCmd::View)?,
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
}

struct ReplCmdHandlerState {
    prompt: String,
    output: String,
    save_file: Option<File>,
}

impl ReplCmdHandler {
    pub fn init(client: ChatGptClient, config: Arc<Config>, role: Option<Role>) -> Result<Self> {
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
            ctrlc,
            state,
        })
    }
    fn handle(&self, cmd: ReplCmd) -> Result<()> {
        match cmd {
            ReplCmd::Input(input) => {
                let mut output = String::new();
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
                self.client.acquire_stream(
                    &input,
                    prompt,
                    &mut output,
                    dump_and_collect,
                    self.ctrlc.clone(),
                )?;
                dump_and_collect(&mut output, "\n\n");
                Config::save_message(self.state.borrow_mut().save_file.as_mut(), &input, &output);
                self.state.borrow_mut().output = output;
            }
            ReplCmd::View => {
                let output = self.state.borrow().output.to_string();
                if output.is_empty() {
                    return Ok(());
                }
                let _ = Editor::new("view ai reply with an external editor")
                    .with_file_extension(".md")
                    .with_predefined_text(&output)
                    .prompt()?;
                dump("", 1);
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

pub enum ReplCmd {
    View,
    UnsetRole,
    Input(String),
    SetRole(String),
}

pub fn dump<T: ToString>(text: T, newlines: usize) {
    print!("{}{}", text.to_string(), "\n".repeat(newlines));
    stdout().flush().unwrap();
}

fn dump_and_collect(output: &mut String, reply: &str) {
    output.push_str(reply);
    dump(reply, 0);
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
