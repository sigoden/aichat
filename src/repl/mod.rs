mod abort;
mod highlighter;
mod prompt;
mod validator;

pub use self::abort::{create_abort_signal, AbortSignal};

use self::highlighter::ReplHighlighter;
use self::prompt::ReplPrompt;
use self::validator::ReplValidator;

use crate::client::init_client;
use crate::config::SharedConfig;
use crate::render::{render_error, render_stream};

use anyhow::{bail, Context, Result};
use arboard::Clipboard;
use crossbeam::sync::WaitGroup;
use fancy_regex::Regex;
use lazy_static::lazy_static;
use reedline::Signal;
use reedline::{
    default_emacs_keybindings, default_vi_insert_keybindings, default_vi_normal_keybindings,
    ColumnarMenu, DefaultCompleter, EditMode, Emacs, KeyCode, KeyModifiers, Keybindings, Reedline,
    ReedlineEvent, ReedlineMenu, Vi,
};
use std::cell::RefCell;
use std::io::Read;

const MENU_NAME: &str = "completion_menu";

const REPL_COMMANDS: [(&str, &str); 14] = [
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

pub struct Repl {
    config: SharedConfig,
    editor: Reedline,
    prompt: ReplPrompt,
    abort: AbortSignal,
    clipboard: std::result::Result<RefCell<Clipboard>, arboard::Error>,
}

impl Repl {
    pub fn init(config: SharedConfig) -> Result<Self> {
        let commands: Vec<String> = REPL_COMMANDS
            .into_iter()
            .map(|(v, _)| v.to_string())
            .collect();

        let completer = Self::create_completer(&config, &commands);
        let highlighter = ReplHighlighter::new(commands, config.clone());
        let menu = Self::create_menu();
        let edit_mode: Box<dyn EditMode> = if config.read().keybindings.is_vi() {
            let mut normal_keybindings = default_vi_normal_keybindings();
            let mut insert_keybindings = default_vi_insert_keybindings();
            Self::extra_keybindings(&mut normal_keybindings);
            Self::extra_keybindings(&mut insert_keybindings);
            Box::new(Vi::new(insert_keybindings, normal_keybindings))
        } else {
            let mut keybindings = default_emacs_keybindings();
            Self::extra_keybindings(&mut keybindings);
            Box::new(Emacs::new(keybindings))
        };
        let mut editor = Reedline::create()
            .with_completer(Box::new(completer))
            .with_highlighter(Box::new(highlighter))
            .with_menu(menu)
            .with_edit_mode(edit_mode)
            .with_quick_completions(true)
            .with_partial_completions(true)
            .with_validator(Box::new(ReplValidator))
            .with_ansi_colors(true);

        editor.enable_bracketed_paste()?;

        let prompt = ReplPrompt::new(config.clone());

        let abort = create_abort_signal();

        let clipboard = Clipboard::new().map(RefCell::new);

        Ok(Self {
            config,
            editor,
            prompt,
            clipboard,
            abort,
        })
    }

    pub fn run(&mut self) -> Result<()> {
        self.banner();

        let mut already_ctrlc = false;

        loop {
            if self.abort.aborted_ctrld() {
                break;
            }
            if self.abort.aborted_ctrlc() && !already_ctrlc {
                already_ctrlc = true;
            }
            let sig = self.editor.read_line(&self.prompt);
            match sig {
                Ok(Signal::Success(line)) => {
                    already_ctrlc = false;
                    self.abort.reset();
                    match self.handle(&line) {
                        Ok(quit) => {
                            if quit {
                                break;
                            }
                        }
                        Err(err) => {
                            render_error(err, self.config.read().highlight);
                        }
                    }
                }
                Ok(Signal::CtrlC) => {
                    self.abort.set_ctrlc();
                    if already_ctrlc {
                        break;
                    }
                    already_ctrlc = true;
                    println!("(To exit, press Ctrl+C again or Ctrl+D or type .exit)\n");
                }
                Ok(Signal::CtrlD) => {
                    self.abort.set_ctrld();
                    break;
                }
                _ => {}
            }
        }
        self.handle(".exit session")?;
        Ok(())
    }

    fn handle(&self, line: &str) -> Result<bool> {
        match parse_command(line) {
            Some((cmd, args)) => match cmd {
                ".help" => {
                    dump_repl_help();
                }
                ".info" => match args {
                    Some("role") => {
                        let info = self.config.read().role_info()?;
                        println!("{}", info);
                    }
                    Some("session") => {
                        let info = self.config.read().session_info()?;
                        println!("{}", info);
                    }
                    Some(_) => unknown_command()?,
                    None => {
                        let output = self.config.read().sys_info()?;
                        println!("{}", output);
                    }
                },
                ".edit" => {
                    if let Some(text) = args {
                        self.ask(text)?;
                    }
                }
                ".model" => match args {
                    Some(name) => {
                        self.config.write().set_model(name)?;
                    }
                    None => println!("Usage: .model <name>"),
                },
                ".role" => match args {
                    Some(name) => {
                        self.config.write().set_role(name)?;
                    }
                    None => println!("Usage: .role <name>"),
                },
                ".session" => {
                    self.config.write().start_session(args)?;
                }
                ".set" => {
                    if let Some(args) = args {
                        self.config.write().update(args)?;
                    }
                }
                ".copy" => {
                    let config = self.config.read();
                    self.copy(config.last_reply())
                        .with_context(|| "Failed to copy the last output")?;
                }
                ".read" => match args {
                    Some(file) => {
                        let mut content = String::new();
                        let mut file =
                            std::fs::File::open(file).with_context(|| "Unable to open file")?;
                        file.read_to_string(&mut content)
                            .with_context(|| "Unable to read file")?;
                        self.ask(&content)?;
                    }
                    None => println!("Usage: .read <textfile>"),
                },
                ".exit" => match args {
                    Some("role") => {
                        self.config.write().clear_role()?;
                    }
                    Some("session") => {
                        self.config.write().end_session()?;
                    }
                    Some(_) => unknown_command()?,
                    None => {
                        return Ok(true);
                    }
                },
                // deprecated this command
                ".clear" => match args {
                    Some("role") => {
                        println!(r#"Deprecated. Use ".exit role" instead."#);
                    }
                    Some("conversation") => {
                        println!(r#"Deprecated. Use ".exit session" instead."#);
                    }
                    _ => unknown_command()?,
                },
                _ => unknown_command()?,
            },
            None => {
                self.ask(line)?;
            }
        }

        println!();

        Ok(false)
    }

    fn ask(&self, input: &str) -> Result<()> {
        if input.is_empty() {
            return Ok(());
        }
        self.config.read().maybe_print_send_tokens(input);
        let wg = WaitGroup::new();
        let client = init_client(self.config.clone())?;
        let ret = render_stream(
            input,
            client.as_ref(),
            &self.config,
            true,
            self.abort.clone(),
            wg.clone(),
        );
        wg.wait();
        let buffer = ret?;
        self.config.write().save_message(input, &buffer)?;
        if self.config.read().auto_copy {
            let _ = self.copy(&buffer);
        }
        Ok(())
    }

    fn banner(&self) {
        let version = env!("CARGO_PKG_VERSION");
        print!(
            r#"Welcome to aichat {version}
Type ".help" for more information.
"#
        )
    }

    fn create_completer(config: &SharedConfig, commands: &[String]) -> DefaultCompleter {
        let mut completion = commands.to_vec();
        completion.extend(config.read().repl_completions());
        let mut completer =
            DefaultCompleter::with_inclusions(&['.', '-', '_', ':']).set_min_word_len(2);
        completer.insert(completion.clone());
        completer
    }

    fn extra_keybindings(keybindings: &mut Keybindings) {
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
            KeyCode::Char('s'),
            ReedlineEvent::Submit,
        );
    }

    fn create_menu() -> ReedlineMenu {
        let completion_menu = ColumnarMenu::default().with_name(MENU_NAME);
        ReedlineMenu::EngineCompleter(Box::new(completion_menu))
    }

    fn copy(&self, text: &str) -> Result<()> {
        if text.is_empty() {
            bail!("No text")
        }
        match self.clipboard.as_ref() {
            Err(err) => bail!("{}", err),
            Ok(clip) => {
                clip.borrow_mut().set_text(text)?;
                Ok(())
            }
        }
    }
}

fn unknown_command() -> Result<()> {
    bail!(r#"Unknown command. Type ".help" for more information."#);
}

fn dump_repl_help() {
    let head = REPL_COMMANDS
        .iter()
        .map(|(name, desc)| format!("{name:<24} {desc}"))
        .collect::<Vec<String>>()
        .join("\n");
    println!(
        r###"{head}

Press Ctrl+C to abort readline, Ctrl+D to exit the REPL"###,
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
