mod completer;
mod highlighter;
mod prompt;

use self::completer::ReplCompleter;
use self::highlighter::ReplHighlighter;
use self::prompt::ReplPrompt;

use crate::client::{call_chat_completions, call_chat_completions_streaming};
use crate::config::{AssertState, Config, GlobalConfig, Input, StateFlags};
use crate::function::need_send_tool_results;
use crate::render::render_error;
use crate::utils::{create_abort_signal, set_text, temp_file, AbortSignal};

use anyhow::{bail, Context, Result};
use fancy_regex::Regex;
use nu_ansi_term::Color;
use reedline::{
    default_emacs_keybindings, default_vi_insert_keybindings, default_vi_normal_keybindings,
    ColumnarMenu, EditCommand, EditMode, Emacs, KeyCode, KeyModifiers, Keybindings, Reedline,
    ReedlineEvent, ReedlineMenu, ValidationResult, Validator, Vi,
};
use reedline::{MenuBuilder, Signal};
use std::{env, process};

lazy_static::lazy_static! {
    static ref SPLIT_FILES_TEXT_ARGS_RE: Regex =
        Regex::new(r"(?m) (-- |--\n|--\r\n|--\r|--$)").unwrap();
}

const MENU_NAME: &str = "completion_menu";

lazy_static::lazy_static! {
    static ref REPL_COMMANDS: [ReplCommand; 33] = [
        ReplCommand::new(".help", "Show this help message", AssertState::pass()),
        ReplCommand::new(".info", "View system info", AssertState::pass()),
        ReplCommand::new(".model", "Change the current LLM", AssertState::pass()),
        ReplCommand::new(
            ".prompt",
            "Create a temporary role using a prompt",
            AssertState::False(StateFlags::SESSION | StateFlags::AGENT)
        ),
        ReplCommand::new(
            ".role",
            "Create or switch to a specific role",
            AssertState::False(StateFlags::SESSION | StateFlags::AGENT)
        ),
        ReplCommand::new(
            ".info role",
            "View role info",
            AssertState::True(StateFlags::ROLE),
        ),
        ReplCommand::new(
            ".edit role",
            "Edit the current role",
            AssertState::TrueFalse(StateFlags::ROLE, StateFlags::SESSION_EMPTY | StateFlags::SESSION),
        ),
        ReplCommand::new(
            ".save role",
            "Save the current role to file",
            AssertState::TrueFalse(StateFlags::ROLE, StateFlags::SESSION_EMPTY | StateFlags::SESSION),
        ),
        ReplCommand::new(
            ".exit role",
            "Leave the role",
            AssertState::True(StateFlags::ROLE),
        ),
        ReplCommand::new(
            ".session",
            "Begin a session",
            AssertState::False(StateFlags::SESSION_EMPTY | StateFlags::SESSION),
        ),
        ReplCommand::new(
            ".clear messages",
            "Erase messages in the current session",
            AssertState::True(StateFlags::SESSION)
        ),
        ReplCommand::new(
            ".info session",
            "View session info",
            AssertState::True(StateFlags::SESSION_EMPTY | StateFlags::SESSION),
        ),
        ReplCommand::new(
            ".edit session",
            "Edit the current session",
            AssertState::True(StateFlags::SESSION_EMPTY | StateFlags::SESSION)
        ),
        ReplCommand::new(
            ".save session",
            "Save the current session to file",
            AssertState::True(StateFlags::SESSION_EMPTY | StateFlags::SESSION)
        ),
        ReplCommand::new(
            ".exit session",
            "End the session",
            AssertState::True(StateFlags::SESSION_EMPTY | StateFlags::SESSION)
        ),
        ReplCommand::new(
            ".rag",
            "Init or use the RAG",
            AssertState::False(StateFlags::AGENT)
        ),
        ReplCommand::new(
            ".rebuild rag",
            "Rebuild the RAG to sync document changes",
            AssertState::True(StateFlags::RAG),
        ),
        ReplCommand::new(
            ".sources rag",
            "View the RAG sources in the last query",
            AssertState::True(StateFlags::RAG),
        ),
        ReplCommand::new(
            ".info rag",
            "View RAG info",
            AssertState::True(StateFlags::RAG),
        ),
        ReplCommand::new(
            ".exit rag",
            "Leave the RAG",
            AssertState::TrueFalse(StateFlags::RAG, StateFlags::AGENT),
        ),
        ReplCommand::new(".agent", "Use a agent", AssertState::bare()),
        ReplCommand::new(
            ".starter",
            "Use the conversation starter",
            AssertState::True(StateFlags::AGENT)
        ),
        ReplCommand::new(
            ".variable",
            "Set agent variable",
            AssertState::True(StateFlags::AGENT)
        ),
        ReplCommand::new(
            ".save agent-config",
            "Save the current agent config to file",
            AssertState::True(StateFlags::AGENT)
        ),
        ReplCommand::new(
            ".info agent",
            "View agent info",
            AssertState::True(StateFlags::AGENT),
        ),
        ReplCommand::new(
            ".exit agent",
            "Leave the agent",
            AssertState::True(StateFlags::AGENT)
        ),
        ReplCommand::new(
            ".file",
            "Include files with the message",
            AssertState::pass()
        ),
        ReplCommand::new(".continue", "Continue the response", AssertState::pass()),
        ReplCommand::new(
            ".regenerate",
            "Regenerate the last response",
            AssertState::pass()
        ),
        ReplCommand::new(".set", "Adjust runtime configuration", AssertState::pass()),
        ReplCommand::new(".delete", "Delete roles/sessions/RAGs/agents", AssertState::pass()),
        ReplCommand::new(".copy", "Copy the last response", AssertState::pass()),
        ReplCommand::new(".exit", "Exit the REPL", AssertState::pass()),
    ];
    static ref COMMAND_RE: Regex = Regex::new(r"^\s*(\.\S*)\s*").unwrap();
    static ref MULTILINE_RE: Regex = Regex::new(r"(?s)^\s*:::\s*(.*)\s*:::\s*$").unwrap();
}

pub struct Repl {
    config: GlobalConfig,
    editor: Reedline,
    prompt: ReplPrompt,
    abort_signal: AbortSignal,
}

impl Repl {
    pub fn init(config: &GlobalConfig) -> Result<Self> {
        let editor = Self::create_editor(config)?;

        let prompt = ReplPrompt::new(config);
        let abort_signal = create_abort_signal();

        Ok(Self {
            config: config.clone(),
            editor,
            prompt,
            abort_signal,
        })
    }

    pub async fn run(&mut self) -> Result<()> {
        self.banner();

        loop {
            if self.abort_signal.aborted_ctrld() {
                break;
            }
            let sig = self.editor.read_line(&self.prompt);
            match sig {
                Ok(Signal::Success(line)) => {
                    self.abort_signal.reset();
                    match self.handle(&line).await {
                        Ok(exit) => {
                            if exit {
                                break;
                            }
                        }
                        Err(err) => {
                            render_error(err, self.config.read().highlight);
                            println!()
                        }
                    }
                }
                Ok(Signal::CtrlC) => {
                    self.abort_signal.set_ctrlc();
                    println!("(To exit, press Ctrl+D or enter \".exit\")\n");
                }
                Ok(Signal::CtrlD) => {
                    self.abort_signal.set_ctrld();
                    break;
                }
                _ => {}
            }
        }
        self.handle(".exit session").await?;
        Ok(())
    }

    async fn handle(&self, mut line: &str) -> Result<bool> {
        if let Ok(Some(captures)) = MULTILINE_RE.captures(line) {
            if let Some(text_match) = captures.get(1) {
                line = text_match.as_str();
            }
        }
        match parse_command(line) {
            Some((cmd, args)) => match cmd {
                ".help" => {
                    dump_repl_help();
                }
                ".info" => match args {
                    Some("role") => {
                        let info = self.config.read().role_info()?;
                        print!("{}", info);
                    }
                    Some("session") => {
                        let info = self.config.read().session_info()?;
                        print!("{}", info);
                    }
                    Some("rag") => {
                        let info = self.config.read().rag_info()?;
                        print!("{}", info);
                    }
                    Some("agent") => {
                        let info = self.config.read().agent_info()?;
                        print!("{}", info);
                    }
                    Some(_) => unknown_command()?,
                    None => {
                        let output = self.config.read().sysinfo()?;
                        print!("{}", output);
                    }
                },
                ".model" => match args {
                    Some(name) => {
                        self.config.write().set_model(name)?;
                    }
                    None => println!("Usage: .model <name>"),
                },
                ".prompt" => match args {
                    Some(text) => {
                        self.config.write().use_prompt(text)?;
                    }
                    None => println!("Usage: .prompt <text>..."),
                },
                ".role" => match args {
                    Some(args) => match args.split_once(['\n', ' ']) {
                        Some((name, text)) => {
                            let role = self.config.read().retrieve_role(name.trim())?;
                            let input = Input::from_str(&self.config, text.trim(), Some(role));
                            ask(&self.config, self.abort_signal.clone(), input, false).await?;
                        }
                        None => {
                            let name = args;
                            if Config::has_role(name) {
                                self.config.write().use_role(name)?;
                            } else {
                                self.config.write().new_role(name)?;
                            }
                        }
                    },
                    None => println!(
                        r#"Usage:
    .role <name>                    # If the role exists, switch to it; otherwise, create a new role
    .role <name> [text]...          # Temporarily switch to the role, send the text, and switch back"#
                    ),
                },
                ".session" => {
                    self.config.write().use_session(args)?;
                }
                ".rag" => {
                    Config::use_rag(&self.config, args, self.abort_signal.clone()).await?;
                }
                ".agent" => match args {
                    Some(name) => {
                        Config::use_agent(&self.config, name, None, self.abort_signal.clone())
                            .await?;
                    }
                    None => println!(r#"Usage: .agent <name>"#),
                },
                ".starter" => match args {
                    Some(value) => {
                        let input = Input::from_str(&self.config, value, None);
                        ask(&self.config, self.abort_signal.clone(), input, true).await?;
                    }
                    None => {
                        let banner = self.config.read().agent_banner()?;
                        self.config.read().print_markdown(&banner)?;
                    }
                },
                ".variable" => match args {
                    Some(args) => {
                        self.config.write().set_agent_variable(args)?;
                    }
                    _ => {
                        println!("Usage: .variable <key> <value>")
                    }
                },
                ".save" => {
                    match args.map(|v| match v.split_once(' ') {
                        Some((subcmd, args)) => (subcmd, Some(args.trim())),
                        None => (v, None),
                    }) {
                        Some(("role", name)) => {
                            self.config.write().save_role(name)?;
                        }
                        Some(("session", name)) => {
                            self.config.write().save_session(name)?;
                        }
                        Some(("agent-config", _)) => {
                            self.config.write().save_agent_config()?;
                        }
                        _ => {
                            println!(r#"Usage: .save <role|session|aegnt-config> [name]"#)
                        }
                    }
                }
                ".edit" => {
                    match args.map(|v| match v.split_once(' ') {
                        Some((subcmd, args)) => (subcmd, Some(args.trim())),
                        None => (v, None),
                    }) {
                        Some(("role", _)) => {
                            self.config.write().edit_role()?;
                        }
                        Some(("session", _)) => {
                            self.config.write().edit_session()?;
                        }
                        _ => {
                            println!(r#"Usage: .edit <role|session>"#)
                        }
                    }
                }
                ".rebuild" => {
                    match args.map(|v| match v.split_once(' ') {
                        Some((subcmd, args)) => (subcmd, Some(args.trim())),
                        None => (v, None),
                    }) {
                        Some(("rag", _)) => {
                            Config::rebuild_rag(&self.config, self.abort_signal.clone()).await?;
                        }
                        _ => {
                            println!(r#"Usage: .rebuild rag"#)
                        }
                    }
                }
                ".sources" => {
                    match args.map(|v| match v.split_once(' ') {
                        Some((subcmd, args)) => (subcmd, Some(args.trim())),
                        None => (v, None),
                    }) {
                        Some(("rag", _)) => {
                            let output = Config::rag_sources(&self.config)?;
                            println!("{}", output);
                        }
                        _ => {
                            println!(r#"Usage: .sources rag"#)
                        }
                    }
                }
                ".file" => match args {
                    Some(args) => {
                        let (files, text) = split_files_text(args);
                        let files = shell_words::split(files).with_context(|| "Invalid args")?;
                        let input = Input::from_files(&self.config, text, files, None).await?;
                        ask(&self.config, self.abort_signal.clone(), input, true).await?;
                    }
                    None => println!("Usage: .file <files>... [-- <text>...]"),
                },
                ".continue" => {
                    let (mut input, output) = match self.config.read().last_message.clone() {
                        Some(v) => v,
                        None => bail!("Unable to continue response"),
                    };
                    input.set_continue_output(&output);
                    ask(&self.config, self.abort_signal.clone(), input, true).await?;
                }
                ".regenerate" => {
                    let (mut input, _) = match self.config.read().last_message.clone() {
                        Some(v) => v,
                        None => bail!("Unable to regenerate the last response"),
                    };
                    input.set_regenerate();
                    ask(&self.config, self.abort_signal.clone(), input, true).await?;
                }
                ".set" => match args {
                    Some(args) => {
                        Config::update(&self.config, args)?;
                    }
                    _ => {
                        println!("Usage: .set <key> <value>...")
                    }
                },
                ".delete" => match args {
                    Some(args) => {
                        Config::delete(&self.config, args)?;
                    }
                    _ => {
                        println!("Usage: .delete [roles|sessions|rags|agents]")
                    }
                },
                ".copy" => {
                    let config = self.config.read();
                    self.copy(config.last_reply())
                        .with_context(|| "Failed to copy the last response")?;
                }
                ".exit" => match args {
                    Some("role") => {
                        self.config.write().exit_role()?;
                    }
                    Some("session") => {
                        self.config.write().exit_session()?;
                    }
                    Some("rag") => {
                        self.config.write().exit_rag()?;
                    }
                    Some("agent") => {
                        self.config.write().exit_agent()?;
                    }
                    Some(_) => unknown_command()?,
                    None => {
                        return Ok(true);
                    }
                },
                ".clear" => match args {
                    Some("messages") => {
                        self.config.write().clear_session_messages()?;
                    }
                    _ => unknown_command()?,
                },
                _ => unknown_command()?,
            },
            None => {
                let input = Input::from_str(&self.config, line, None);
                ask(&self.config, self.abort_signal.clone(), input, true).await?;
            }
        }

        println!();

        Ok(false)
    }

    fn banner(&self) {
        let name = env!("CARGO_CRATE_NAME");
        let version = env!("CARGO_PKG_VERSION");
        print!(
            r#"Welcome to {name} {version}
Type ".help" for additional help.
"#
        )
    }

    fn create_editor(config: &GlobalConfig) -> Result<Reedline> {
        let completer = ReplCompleter::new(config);
        let highlighter = ReplHighlighter::new(config);
        let menu = Self::create_menu();
        let edit_mode = Self::create_edit_mode(config);
        let mut editor = Reedline::create()
            .with_completer(Box::new(completer))
            .with_highlighter(Box::new(highlighter))
            .with_menu(menu)
            .with_edit_mode(edit_mode)
            .with_quick_completions(true)
            .with_partial_completions(true)
            .use_bracketed_paste(true)
            .with_validator(Box::new(ReplValidator))
            .with_ansi_colors(true);

        if let Ok(cmd) = config.read().editor() {
            let temp_file = temp_file("-repl-", ".txt");
            let command = process::Command::new(cmd);
            editor = editor.with_buffer_editor(command, temp_file);
        }

        Ok(editor)
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
            KeyModifiers::SHIFT,
            KeyCode::BackTab,
            ReedlineEvent::MenuPrevious,
        );
        keybindings.add_binding(
            KeyModifiers::CONTROL,
            KeyCode::Enter,
            ReedlineEvent::Edit(vec![EditCommand::InsertNewline]),
        );
        keybindings.add_binding(
            KeyModifiers::SHIFT,
            KeyCode::Enter,
            ReedlineEvent::Edit(vec![EditCommand::InsertNewline]),
        );
        keybindings.add_binding(
            KeyModifiers::ALT,
            KeyCode::Enter,
            ReedlineEvent::Edit(vec![EditCommand::InsertNewline]),
        );
    }

    fn create_edit_mode(config: &GlobalConfig) -> Box<dyn EditMode> {
        let edit_mode: Box<dyn EditMode> = if config.read().keybindings == "vi" {
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
        edit_mode
    }

    fn create_menu() -> ReedlineMenu {
        let completion_menu = ColumnarMenu::default().with_name(MENU_NAME);
        ReedlineMenu::EngineCompleter(Box::new(completion_menu))
    }

    fn copy(&self, text: &str) -> Result<()> {
        if text.is_empty() {
            bail!("Empty text")
        }
        set_text(text)?;
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct ReplCommand {
    name: &'static str,
    description: &'static str,
    state: AssertState,
}

impl ReplCommand {
    fn new(name: &'static str, desc: &'static str, state: AssertState) -> Self {
        Self {
            name,
            description: desc,
            state,
        }
    }

    fn is_valid(&self, flags: StateFlags) -> bool {
        match self.state {
            AssertState::True(true_flags) => true_flags & flags != StateFlags::empty(),
            AssertState::False(false_flags) => false_flags & flags == StateFlags::empty(),
            AssertState::TrueFalse(true_flags, false_flags) => {
                (true_flags & flags != StateFlags::empty())
                    && (false_flags & flags == StateFlags::empty())
            }
            AssertState::Equal(check_flags) => check_flags == flags,
        }
    }
}

/// A default validator which checks for mismatched quotes and brackets
struct ReplValidator;

impl Validator for ReplValidator {
    fn validate(&self, line: &str) -> ValidationResult {
        let line = line.trim();
        if line.starts_with(r#":::"#) && !line[3..].ends_with(r#":::"#) {
            ValidationResult::Incomplete
        } else {
            ValidationResult::Complete
        }
    }
}

#[async_recursion::async_recursion]
async fn ask(
    config: &GlobalConfig,
    abort_signal: AbortSignal,
    mut input: Input,
    with_embeddings: bool,
) -> Result<()> {
    if input.is_empty() {
        return Ok(());
    }
    if with_embeddings {
        input.use_embeddings(abort_signal.clone()).await?;
    }
    while config.read().is_compressing_session() {
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }

    let client = input.create_client()?;
    config.write().before_chat_completion(&input)?;
    let (output, tool_results) = if config.read().stream {
        call_chat_completions_streaming(&input, client.as_ref(), config, abort_signal.clone())
            .await?
    } else {
        call_chat_completions(&input, client.as_ref(), config).await?
    };
    config
        .write()
        .after_chat_completion(&input, &output, &tool_results)?;
    if need_send_tool_results(&tool_results) {
        ask(
            config,
            abort_signal,
            input.merge_tool_call(output, tool_results),
            false,
        )
        .await
    } else {
        if config.write().should_compress_session() {
            let config = config.clone();
            let color = if config.read().light_theme {
                Color::LightGray
            } else {
                Color::DarkGray
            };
            print!(
                "\n📢 {}\n",
                color.italic().paint("Compressing the session."),
            );
            tokio::spawn(async move {
                let _ = compress_session(&config).await;
                config.write().end_compressing_session();
            });
        }
        Ok(())
    }
}

fn unknown_command() -> Result<()> {
    bail!(r#"Unknown command. Type ".help" for additional help."#);
}

fn dump_repl_help() {
    let head = REPL_COMMANDS
        .iter()
        .map(|cmd| format!("{:<24} {}", cmd.name, cmd.description))
        .collect::<Vec<String>>()
        .join("\n");
    println!(
        r###"{head}

Type ::: to start multi-line editing, type ::: to finish it.
Press Ctrl+O to open an editor for editing the input buffer.
Press Ctrl+C to cancel the response, Ctrl+D to exit the REPL."###,
    );
}

fn parse_command(line: &str) -> Option<(&str, Option<&str>)> {
    match COMMAND_RE.captures(line) {
        Ok(Some(captures)) => {
            let cmd = captures.get(1)?.as_str();
            let args = line[captures[0].len()..].trim();
            let args = if args.is_empty() { None } else { Some(args) };
            Some((cmd, args))
        }
        _ => None,
    }
}

async fn compress_session(config: &GlobalConfig) -> Result<()> {
    let input = Input::from_str(config, config.read().summarize_prompt(), None);
    let client = input.create_client()?;
    let summary = client.chat_completions(input).await?.text;
    config.write().compress_session(&summary);
    Ok(())
}

fn split_files_text(args: &str) -> (&str, &str) {
    match SPLIT_FILES_TEXT_ARGS_RE.find(args).ok().flatten() {
        Some(mat) => {
            let files = &args[0..mat.start()];
            let text = if mat.end() < args.len() {
                &args[mat.end()..]
            } else {
                ""
            };
            (files, text)
        }
        None => (args, ""),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_process_command_line() {
        assert_eq!(parse_command(" ."), Some((".", None)));
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

    #[test]
    fn test_split_files_text() {
        assert_eq!(split_files_text("file.txt"), ("file.txt", ""));
        assert_eq!(split_files_text("file.txt --"), ("file.txt", ""));
        assert_eq!(split_files_text("file.txt -- hello"), ("file.txt", "hello"));
        assert_eq!(
            split_files_text("file.txt --\nhello"),
            ("file.txt", "hello")
        );
        assert_eq!(
            split_files_text("file.txt --\r\nhello"),
            ("file.txt", "hello")
        );
        assert_eq!(
            split_files_text("file.txt --\rhello"),
            ("file.txt", "hello")
        );
    }
}
