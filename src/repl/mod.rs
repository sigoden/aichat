mod abort;
mod handler;
mod highlighter;
mod init;
mod prompt;

pub use self::abort::*;
pub use self::handler::*;
pub use self::init::Repl;

use crate::client::ChatGptClient;
use crate::config::SharedConfig;
use crate::print_now;
use crate::term;

use anyhow::{Context, Result};
use reedline::Signal;
use std::borrow::Cow;
use std::sync::Arc;

pub const REPL_COMMANDS: [(&str, &str); 11] = [
    (".info", "Print the information"),
    (".set", "Modify the configuration temporarily"),
    (".prompt", "Add a GPT prompt"),
    (".role", "Select a role"),
    (".clear role", "Clear the currently selected role"),
    (".conversation", "Start a conversation."),
    (".clear conversation", "End current conversation."),
    (".history", "Print the history"),
    (".clear history", "Clear the history"),
    (".help", "Print this help message"),
    (".exit", "Exit the REPL"),
];

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
        let line = clean_multiline_symbols(&line);
        match parse_command(&line) {
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
                        let history = Box::new(self.editor.history_mut());
                        history.clear().with_context(|| "Failed to clear history")?;
                        print_now!("\n");
                    }
                    Some("role") => handler.handle(ReplCmd::ClearRole)?,
                    Some("conversation") => handler.handle(ReplCmd::EndConversatoin)?,
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
                    handler.handle(ReplCmd::ViewInfo)?;
                }
                ".set" => {
                    handler.handle(ReplCmd::UpdateConfig(args.unwrap_or_default().to_string()))?;
                    self.prompt.sync_config();
                }
                ".prompt" => {
                    let text = args.unwrap_or_default().to_string();
                    if text.is_empty() {
                        print_now!("Usage: .prompt <text>.\n\n");
                    } else {
                        handler.handle(ReplCmd::Prompt(text))?;
                    }
                }
                ".conversation" => {
                    handler.handle(ReplCmd::StartConversation)?;
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

Type `{{` to enter the multi-line editing mode, type '}}' to exit the mode.
Press Ctrl+C to abort readline, Ctrl+D to exit the REPL

"###,
    );
}

fn clean_multiline_symbols(line: &str) -> Cow<str> {
    let trimed_line = line.trim();
    match trimed_line.chars().next() {
        Some('{') | Some('[') | Some('(') => trimed_line[1..trimed_line.len() - 1].into(),
        _ => Cow::Borrowed(line),
    }
}

fn parse_command(line: &str) -> Option<(&str, Option<&str>)> {
    let mut trimed_line = line.trim_start();
    if trimed_line.starts_with('.') {
        trimed_line = trimed_line.trim_end();
        match trimed_line
            .split_once(' ')
            .or_else(|| trimed_line.split_once('\n'))
        {
            Some((head, tail)) => {
                let trimed_tail = tail.trim();
                if trimed_tail.is_empty() {
                    Some((head, None))
                } else {
                    Some((head, Some(trimed_tail)))
                }
            }
            None => Some((trimed_line, None)),
        }
    } else {
        None
    }
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
    }
}
