mod abort;
mod handler;
mod init;

use crate::client::ChatGptClient;
use crate::config::SharedConfig;
use crate::print_now;
use crate::term;

use anyhow::{Context, Result};
use reedline::{DefaultPrompt, Reedline, Signal};
use std::sync::Arc;

pub use self::abort::*;
pub use self::handler::*;

pub const REPL_COMMANDS: [(&str, &str, bool); 10] = [
    (".info", "Print the information", false),
    (".set", "Modify the configuration temporarily", false),
    (".prompt", "Add a GPT prompt", true),
    (".role", "Select a role", false),
    (".clear role", "Clear the currently selected role", false),
    (".history", "Print the history", false),
    (".clear history", "Clear the history", false),
    (".editor", "Enter editor mode for multiline input", true),
    (".help", "Print this help message", false),
    (".exit", "Exit the REPL", false),
];

pub struct Repl {
    editor: Reedline,
    prompt: DefaultPrompt,
}

impl Repl {
    pub fn run(&mut self, client: ChatGptClient, config: SharedConfig) -> Result<()> {
        let abort = AbortSignal::new();
        let handler = ReplCmdHandler::init(client, config, abort.clone())?;
        print_now!("Welcome to aichat {}\n", env!("CARGO_PKG_VERSION"));
        print_now!("Type \".help\" for more information.\n");
        let mut already_ctrlc = false;
        let handler = Arc::new(handler);
        loop {
            if abort.aborted_ctrld() {
                break;
            }
            if abort.aborted_ctrlc() && !already_ctrlc {
                already_ctrlc = true;
            }
            let sig = self.editor.read_line(&self.prompt);
            match sig {
                Ok(Signal::Success(line)) => {
                    already_ctrlc = false;
                    abort.reset();
                    match self.handle_line(handler.clone(), line) {
                        Ok(quit) => {
                            if quit {
                                break;
                            }
                        }
                        Err(err) => {
                            let err = format!("{err:?}");
                            print_now!("{}\n\n", err.trim());
                        }
                    }
                }
                Ok(Signal::CtrlC) => {
                    abort.set_ctrlc();
                    if !already_ctrlc {
                        already_ctrlc = true;
                        print_now!("(To exit, press Ctrl+C again or Ctrl+D or type .exit)\n\n");
                    } else {
                        break;
                    }
                }
                Ok(Signal::CtrlD) => {
                    abort.set_ctrld();
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
                        print_now!("\n");
                    }
                    Some("role") => handler.handle(ReplCmd::ClearRole)?,
                    _ => dump_unknown_command(),
                },
                ".history" => {
                    self.editor.print_history()?;
                    print_now!("\n");
                }
                ".role" => match args {
                    Some(name) => handler.handle(ReplCmd::SetRole(name.to_string()))?,
                    None => print_now!("Usage: .role <name>\n\n"),
                },
                ".info" => {
                    handler.handle(ReplCmd::Info)?;
                }
                ".editor" => {
                    let mut text = args.unwrap_or_default().to_string();
                    if text.is_empty() {
                        print_now!("Usage: .editor {{ <your multiline/paste content here> }}\n\n");
                    } else {
                        if text.starts_with('{') && text.ends_with('}') {
                            text = text[1..text.len() - 1].to_string()
                        }
                        handler.handle(ReplCmd::Submit(text))?;
                    }
                }
                ".set" => {
                    handler.handle(ReplCmd::UpdateConfig(args.unwrap_or_default().to_string()))?
                }
                ".prompt" => {
                    let mut text = args.unwrap_or_default().to_string();
                    if text.is_empty() {
                        print_now!("Usage: .prompt {{ <your content here> }}.\n\n");
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
}

fn dump_unknown_command() {
    print_now!("Error: Unknown command. Type \".help\" for more information.\n\n");
}

fn dump_repl_help() {
    let head = REPL_COMMANDS
        .iter()
        .map(|(name, desc, _)| format!("{name:<15} {desc}"))
        .collect::<Vec<String>>()
        .join("\n");
    print_now!(
        "{}\n\nPress Ctrl+C to abort session, Ctrl+D to exit the REPL\n\n",
        head,
    );
}
