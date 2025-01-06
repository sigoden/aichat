mod completer;
mod highlighter;
mod prompt;

use self::completer::ReplCompleter;
use self::highlighter::ReplHighlighter;
use self::prompt::ReplPrompt;

use crate::client::{call_chat_completions, call_chat_completions_streaming};
use crate::config::{AssertState, Config, GlobalConfig, Input, StateFlags};
use crate::render::render_error;
use crate::utils::{
    abortable_run_with_spinner, create_abort_signal, set_text, temp_file, AbortSignal,
};

use anyhow::{bail, Context, Result};
use fancy_regex::Regex;
use reedline::{
    default_emacs_keybindings, default_vi_insert_keybindings, default_vi_normal_keybindings,
    ColumnarMenu, EditCommand, EditMode, Emacs, KeyCode, KeyModifiers, Keybindings, Reedline,
    ReedlineEvent, ReedlineMenu, ValidationResult, Validator, Vi,
};
use reedline::{MenuBuilder, Signal};
use std::{env, process};

const MENU_NAME: &str = "completion_menu";

lazy_static::lazy_static! {
    static ref REPL_COMMANDS: [ReplCommand; 34] = [
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
            AssertState::TrueFalse(StateFlags::ROLE, StateFlags::SESSION),
        ),
        ReplCommand::new(
            ".save role",
            "Save the current role to file",
            AssertState::TrueFalse(StateFlags::ROLE, StateFlags::SESSION_EMPTY | StateFlags::SESSION),
        ),
        ReplCommand::new(
            ".exit role",
            "Leave the role",
            AssertState::TrueFalse(StateFlags::ROLE, StateFlags::SESSION),
        ),
        ReplCommand::new(
            ".session",
            "Begin a session",
            AssertState::False(StateFlags::SESSION_EMPTY | StateFlags::SESSION),
        ),
        ReplCommand::new(
            ".empty session",
            "Erase messages in the current session",
            AssertState::True(StateFlags::SESSION)
        ),
        ReplCommand::new(
            ".compress session",
            "Compress messages in the current session",
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
        ReplCommand::new(".agent", "Use a agent", AssertState::bare()),
        ReplCommand::new(
            ".starter",
            "Use the conversation starter",
            AssertState::True(StateFlags::AGENT)
        ),
        ReplCommand::new(
            ".variable",
            "Set agent variable",
            AssertState::TrueFalse(StateFlags::AGENT, StateFlags::SESSION)
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
            ".rag",
            "Init or use the RAG",
            AssertState::False(StateFlags::AGENT)
        ),
        ReplCommand::new(
            ".edit rag-docs",
            "Edit the RAG documents",
            AssertState::TrueFalse(StateFlags::RAG, StateFlags::AGENT),
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
        ReplCommand::new(
            ".file",
            "Include files, directories, URLs or commands",
            AssertState::pass()
        ),
        ReplCommand::new(".continue", "Continue the response", AssertState::pass()),
        ReplCommand::new(
            ".regenerate",
            "Regenerate the last response",
            AssertState::pass()
        ),
        ReplCommand::new(".copy", "Copy the last response", AssertState::pass()),
        ReplCommand::new(".set", "Adjust runtime configuration", AssertState::pass()),
        ReplCommand::new(".delete", "Delete roles/sessions/RAGs/agents", AssertState::pass()),
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
        if AssertState::False(StateFlags::AGENT | StateFlags::RAG)
            .assert(self.config.read().state())
        {
            self.banner();
        }

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
                            render_error(err);
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
        self.config.write().exit_session()?;
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
                            let input = Input::from_str(&self.config, text, Some(role));
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
                    Config::maybe_autoname_session(self.config.clone());
                }
                ".rag" => {
                    Config::use_rag(&self.config, args, self.abort_signal.clone()).await?;
                }
                ".agent" => match split_args(args) {
                    Some((agent_name, session_name)) => {
                        Config::use_agent(
                            &self.config,
                            agent_name,
                            session_name,
                            self.abort_signal.clone(),
                        )
                        .await?;
                    }
                    None => println!(r#"Usage: .agent <agent-name> [session-name]"#),
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
                ".save" => match split_args(args) {
                    Some(("role", name)) => {
                        self.config.write().save_role(name)?;
                    }
                    Some(("session", name)) => {
                        self.config.write().save_session(name)?;
                    }
                    _ => {
                        println!(r#"Usage: .save <role|session> [name]"#)
                    }
                },
                ".edit" => match args {
                    Some("role") => {
                        self.config.write().edit_role()?;
                    }
                    Some("session") => {
                        self.config.write().edit_session()?;
                    }
                    Some("rag-docs") => {
                        Config::edit_rag_docs(&self.config, self.abort_signal.clone()).await?;
                    }
                    _ => {
                        println!(r#"Usage: .edit <role|session|rag-docs>"#)
                    }
                },
                ".compress" => match args {
                    Some("session") => {
                        abortable_run_with_spinner(
                            Config::compress_session(&self.config),
                            "Compressing",
                            self.abort_signal.clone(),
                        )
                        .await?;
                        println!("✓ Successfully compressed the session.");
                    }
                    _ => {
                        println!(r#"Usage: .compress session"#)
                    }
                },
                ".empty" => match args {
                    Some("session") => {
                        self.config.write().empty_session()?;
                    }
                    _ => {
                        println!(r#"Usage: .empty session"#)
                    }
                },
                ".rebuild" => match args {
                    Some("rag") => {
                        Config::rebuild_rag(&self.config, self.abort_signal.clone()).await?;
                    }
                    _ => {
                        println!(r#"Usage: .rebuild rag"#)
                    }
                },
                ".sources" => match args {
                    Some("rag") => {
                        let output = Config::rag_sources(&self.config)?;
                        println!("{}", output);
                    }
                    _ => {
                        println!(r#"Usage: .sources rag"#)
                    }
                },
                ".file" => match args {
                    Some(args) => {
                        let (files, text) = split_files_text(args, cfg!(windows));
                        let input = Input::from_files_with_spinner(
                            &self.config,
                            text,
                            files,
                            None,
                            self.abort_signal.clone(),
                        )
                        .await?;
                        ask(&self.config, self.abort_signal.clone(), input, true).await?;
                    }
                    None => println!(
                        r#"Usage: .file <file|dir|url|cmd>... [-- <text>...]

.file /tmp/file.txt
.file src/ Cargo.toml -- analyze
.file https://example.com/file.txt -- summarize
.file https://example.com/image.png -- recognize text
.file `git diff` -- Generate git commit message"#
                    ),
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
                        println!("Usage: .delete <role|session|rag|agent-data>")
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
                        if self.config.read().agent.is_some() {
                            self.config.write().exit_agent_session()?;
                        } else {
                            self.config.write().exit_session()?;
                        }
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
                        bail!("Use '.empty session' instead");
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
            bail!("No text to copy")
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
        self.state.assert(flags)
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
    let (output, tool_results) = if input.stream() {
        call_chat_completions_streaming(&input, client.as_ref(), abort_signal.clone()).await?
    } else {
        call_chat_completions(&input, false, client.as_ref(), abort_signal.clone()).await?
    };
    config
        .write()
        .after_chat_completion(&input, &output, &tool_results)?;
    if !tool_results.is_empty() {
        ask(
            config,
            abort_signal,
            input.merge_tool_results(output, tool_results),
            false,
        )
        .await
    } else {
        Config::maybe_autoname_session(config.clone());
        Config::maybe_compress_session(config.clone());
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

fn split_args(args: Option<&str>) -> Option<(&str, Option<&str>)> {
    args.map(|v| match v.split_once(' ') {
        Some((subcmd, args)) => (subcmd, Some(args.trim())),
        None => (v, None),
    })
}

fn split_files_text(line: &str, is_win: bool) -> (Vec<String>, &str) {
    let mut words = Vec::new();
    let mut word = String::new();
    let mut unbalance: Option<char> = None;
    let mut prev_char: Option<char> = None;
    let mut text_starts_at = None;
    let unquote_word = |word: &str| {
        if ((word.starts_with('"') && word.ends_with('"'))
            || (word.starts_with('\'') && word.ends_with('\'')))
            && word.len() >= 2
        {
            word[1..word.len() - 1].to_string()
        } else {
            word.to_string()
        }
    };
    let chars: Vec<char> = line.chars().collect();

    for (i, char) in chars.iter().cloned().enumerate() {
        match unbalance {
            Some(ub_char) if ub_char == char => {
                word.push(char);
                unbalance = None;
            }
            Some(_) => {
                word.push(char);
            }
            None => match char {
                ' ' | '\t' | '\r' | '\n' => {
                    if char == '\r' && chars.get(i + 1) == Some(&'\n') {
                        continue;
                    }
                    if let Some('\\') = prev_char.filter(|_| !is_win) {
                        word.push(char);
                    } else if !word.is_empty() {
                        if word == "--" {
                            word.clear();
                            text_starts_at = Some(i + 1);
                            break;
                        }
                        words.push(unquote_word(&word));
                        word.clear();
                    }
                }
                '\'' | '"' | '`' => {
                    word.push(char);
                    unbalance = Some(char);
                }
                '\\' => {
                    if is_win || prev_char.map(|c| c == '\\').unwrap_or_default() {
                        word.push(char);
                    }
                }
                _ => {
                    word.push(char);
                }
            },
        }
        prev_char = Some(char);
    }

    if !word.is_empty() && word != "--" {
        words.push(unquote_word(&word));
    }
    let text = match text_starts_at {
        Some(start) => &line[start..],
        None => "",
    };

    (words, text)
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
        assert_eq!(
            split_files_text("file.txt", false),
            (vec!["file.txt".into()], "")
        );
        assert_eq!(
            split_files_text("file.txt --", false),
            (vec!["file.txt".into()], "")
        );
        assert_eq!(
            split_files_text("file.txt -- hello", false),
            (vec!["file.txt".into()], "hello")
        );
        assert_eq!(
            split_files_text("file.txt -- \thello", false),
            (vec!["file.txt".into()], "\thello")
        );
        assert_eq!(
            split_files_text("file.txt --\nhello", false),
            (vec!["file.txt".into()], "hello")
        );
        assert_eq!(
            split_files_text("file.txt --\r\nhello", false),
            (vec!["file.txt".into()], "hello")
        );
        assert_eq!(
            split_files_text("file.txt --\rhello", false),
            (vec!["file.txt".into()], "hello")
        );
        assert_eq!(
            split_files_text(r#"file1.txt 'file2.txt' "file3.txt""#, false),
            (
                vec!["file1.txt".into(), "file2.txt".into(), "file3.txt".into()],
                ""
            )
        );
        assert_eq!(
            split_files_text(r#"./file1.txt 'file1 - Copy.txt' file\ 2.txt"#, false),
            (
                vec![
                    "./file1.txt".into(),
                    "file1 - Copy.txt".into(),
                    "file 2.txt".into()
                ],
                ""
            )
        );
        assert_eq!(
            split_files_text(r#".\file.txt C:\dir\file.txt"#, true),
            (vec![".\\file.txt".into(), "C:\\dir\\file.txt".into()], "")
        );
    }
}
