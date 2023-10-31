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

use anyhow::Result;
use fancy_regex::Regex;
use lazy_static::lazy_static;
use reedline::Signal;
use std::rc::Rc;

pub const REPL_COMMANDS: [(&str, &str); 14] = [
    (".help", "Print this help message"),
    (".info", "Print system info"),
    (".edit", "Multi-line editing (CTRL+S to finish)"),
    (".model", "Switch LLM model"),
    (".role", "Use role"),
    (".info role", "Show role info"),
    (".exit role", "Leave current role"),
    (".session", "Start a context-aware chat session"),
    (".info session", "Show session info"),
    (".exit session", "End the current session"),
    (".set", "Modify the configuration parameters"),
    (".copy", "Copy the last reply to the clipboard"),
    (".read", "Import from file and submit"),
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
                            print_now!("Error: {}\n\n", err.trim());
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
        handler.handle(ReplCmd::ExitSession)?;
        Ok(())
    }

    fn handle_line(&mut self, handler: &Rc<ReplCmdHandler>, line: &str) -> Result<bool> {
        match parse_command(line) {
            Some((cmd, args)) => match cmd {
                ".help" => {
                    dump_repl_help();
                }
                ".info" => match args {
                    Some("role") => handler.handle(ReplCmd::RoleInfo)?,
                    Some("session") => handler.handle(ReplCmd::SessionInfo)?,
                    Some(_) => unknown_command(),
                    None => {
                        handler.handle(ReplCmd::Info)?;
                    }
                },
                ".edit" => {
                    if let Some(text) = args {
                        handler.handle(ReplCmd::Submit(text.to_string()))?;
                    }
                }
                ".model" => match args {
                    Some(name) => handler.handle(ReplCmd::SetModel(name.to_string()))?,
                    None => print_now!("Usage: .model <name>\n\n"),
                },
                ".role" => match args {
                    Some(name) => handler.handle(ReplCmd::SetRole(name.to_string()))?,
                    None => print_now!("Usage: .role <name>\n\n"),
                },
                ".session" => {
                    handler.handle(ReplCmd::StartSession(args.map(|v| v.to_string())))?;
                }
                ".set" => {
                    handler.handle(ReplCmd::Set(args.unwrap_or_default().to_string()))?;
                }
                ".copy" => {
                    handler.handle(ReplCmd::Copy)?;
                }
                ".read" => match args {
                    Some(file) => handler.handle(ReplCmd::ReadFile(file.to_string()))?,
                    None => print_now!("Usage: .read <file name>\n\n"),
                },
                ".exit" => match args {
                    Some("role") => handler.handle(ReplCmd::ExitRole)?,
                    Some("session") => handler.handle(ReplCmd::ExitSession)?,
                    Some(_) => unknown_command(),
                    None => {
                        return Ok(true);
                    }
                },
                // deprecated
                ".clear" => match args {
                    Some("role") => {
                        print_now!("Deprecated. Use '.exit role' instead.\n\n");
                    }
                    Some("session") => {
                        print_now!("Deprecated. Use '.exit session' instead.\n\n");
                    }
                    _ => unknown_command(),
                },
                _ => unknown_command(),
            },
            None => {
                handler.handle(ReplCmd::Submit(line.to_string()))?;
            }
        }

        Ok(false)
    }
}

fn unknown_command() {
    print_now!("Unknown command. Try `.help`.\n\n");
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
