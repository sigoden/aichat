mod abort;
mod handler;
mod highlighter;
mod init;
mod prompt;
mod validator;

pub use self::abort::*;
pub use self::handler::*;
pub use self::init::Repl;

use crate::config::SharedConfig;
use crate::print_now;
use crate::term;

use anyhow::{Context, Result};
use fancy_regex::Regex;
use lazy_static::lazy_static;
use reedline::Signal;
use std::rc::Rc;

pub const REPL_COMMANDS: [(&str, &str); 14] = [
    (".info", "Print system-wide information"),
    (".set", "Modify the configuration temporarily"),
    (".model", "Choose a model"),
    (".role", "Select a role"),
    (".clear role", "Clear the currently selected role"),
    (".session", "Start a session"),
    (".clear session", "End current session"),
    (".copy", "Copy the last output to the clipboard"),
    (".read", "Read the contents of a file and submit"),
    (".edit", "Multi-line editing (CTRL+S to finish)"),
    (".history", "Print the REPL history"),
    (".clear history", "Clear the REPL history"),
    (".help", "Print this help message"),
    (".exit", "Exit the REPL"),
];

lazy_static! {
    static ref COMMAND_RE: Regex = Regex::new(r"^\s*(\.\S+)\s*").unwrap();
    static ref EDIT_RE: Regex = Regex::new(r"^\s*\.edit\s*").unwrap();
}

impl Repl {
    pub fn run(&mut self, config: SharedConfig) -> Result<()> {
        let abort = AbortSignal::new();
        let handler = ReplCmdHandler::init(config, abort.clone())?;
        print_now!("Welcome to aichat {}\n", env!("CARGO_PKG_VERSION"));
        print_now!("Type \".help\" for more information.\n");
        let mut already_ctrlc = false;
        let handler = Rc::new(handler);
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
                    match self.handle_line(&handler, &line) {
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
                    if already_ctrlc {
                        break;
                    }
                    already_ctrlc = true;
                    print_now!("(To exit, press Ctrl+C again or Ctrl+D or type .exit)\n\n");
                }
                Ok(Signal::CtrlD) => {
                    abort.set_ctrld();
                    break;
                }
                _ => {}
            }
        }
        handler.handle(ReplCmd::EndSession)?;
        Ok(())
    }

    fn handle_line(&mut self, handler: &Rc<ReplCmdHandler>, line: &str) -> Result<bool> {
        match parse_command(line) {
            Some((cmd, args)) => match cmd {
                ".exit" => {
                    return Ok(true);
                }
                ".help" => {
                    dump_repl_help();
                }
                ".clear" => match args {
                    Some("screen") => term::clear_screen(0)?,
                    Some("history") => {
                        self.editor
                            .history_mut()
                            .clear()
                            .with_context(|| "Failed to clear history")?;
                        print_now!("\n");
                    }
                    Some("role") => handler.handle(ReplCmd::ClearRole)?,
                    Some("session") => handler.handle(ReplCmd::EndSession)?,
                    _ => dump_unknown_command(),
                },
                ".history" => {
                    self.editor.print_history()?;
                    print_now!("\n");
                }
                ".model" => match args {
                    Some(name) => handler.handle(ReplCmd::SetModel(name.to_string()))?,
                    None => print_now!("Usage: .model <name>\n\n"),
                },
                ".role" => match args {
                    Some(name) => handler.handle(ReplCmd::SetRole(name.to_string()))?,
                    None => print_now!("Usage: .role <name>\n\n"),
                },
                ".info" => {
                    handler.handle(ReplCmd::ViewInfo)?;
                }
                ".set" => {
                    handler.handle(ReplCmd::UpdateConfig(args.unwrap_or_default().to_string()))?;
                    self.prompt.sync_config();
                }
                ".session" => {
                    handler.handle(ReplCmd::StartSession(args.map(|v| v.to_string())))?;
                }
                ".copy" => {
                    handler.handle(ReplCmd::Copy)?;
                }
                ".read" => match args {
                    Some(file) => handler.handle(ReplCmd::ReadFile(file.to_string()))?,
                    None => print_now!("Usage: .read <file name>\n\n"),
                },
                ".edit" => {
                    if let Some(text) = args {
                        handler.handle(ReplCmd::Submit(text.to_string()))?;
                    }
                }
                _ => dump_unknown_command(),
            },
            None => {
                handler.handle(ReplCmd::Submit(line.to_string()))?;
            }
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
        .map(|(name, desc)| format!("{name:<24} {desc}"))
        .collect::<Vec<String>>()
        .join("\n");
    print_now!(
        r###"{head}

Press Ctrl+C to abort readline, Ctrl+D to exit the REPL

"###,
    );
}

fn parse_command(line: &str) -> Option<(&str, Option<&str>)> {
    if let Ok(Some(captures)) = COMMAND_RE.captures(line) {
        if let Some(cmd) = captures.get(1) {
            let cmd = cmd.as_str();
            let args = line[captures[0].len()..].trim();
            let args = if args.is_empty() { None } else { Some(args) };
            return Some((cmd, args));
        }
    }
    None
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_process_command_line() {
        assert_eq!(parse_command(" .role"), Some((".role", None)));
        assert_eq!(parse_command(" .role  "), Some((".role", None)));
        assert_eq!(
            parse_command(" .set dry_run true"),
            Some((".set", Some("dry_run true")))
        );
        assert_eq!(
            parse_command(" .set dry_run true  "),
            Some((".set", Some("dry_run true")))
        );
        assert_eq!(
            parse_command(".prompt \nabc\n"),
            Some((".prompt", Some("abc")))
        );
        assert_eq!(
            parse_command(".edit\r\nabc\r\n"),
            Some((".edit", Some("abc")))
        );
    }
}
