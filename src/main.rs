mod cli;
mod client;
mod config;
mod function;
mod rag;
mod render;
mod repl;
mod serve;
#[macro_use]
mod utils;

#[macro_use]
extern crate log;

use crate::cli::Cli;
use crate::client::{
    call_chat_completions, call_chat_completions_streaming, list_chat_models, ChatCompletionsOutput,
};
use crate::config::{
    ensure_parent_exists, list_agents, load_env_file, Config, GlobalConfig, Input, WorkingMode,
    CODE_ROLE, EXPLAIN_SHELL_ROLE, SHELL_ROLE, TEMP_SESSION_NAME,
};
use crate::function::{eval_tool_calls, need_send_tool_results};
use crate::render::render_error;
use crate::repl::Repl;
use crate::utils::*;

use anyhow::{bail, Result};
use clap::Parser;
use inquire::validator::Validation;
use inquire::Text;
use is_terminal::IsTerminal;
use parking_lot::RwLock;
use simplelog::{format_description, ConfigBuilder, LevelFilter, SimpleLogger, WriteLogger};
use std::{
    env,
    io::{stderr, stdin, Read},
    process,
    sync::Arc,
};

#[tokio::main]
async fn main() -> Result<()> {
    load_env_file()?;
    let cli = Cli::parse();
    let text = cli.text();
    let text = aggregate_text(text)?;
    let working_mode = if cli.serve.is_some() {
        WorkingMode::Serve
    } else if text.is_none() && cli.file.is_empty() {
        WorkingMode::Repl
    } else {
        WorkingMode::Command
    };
    setup_logger(working_mode.is_serve())?;
    let config = Arc::new(RwLock::new(Config::init(working_mode)?));
    let highlight = config.read().highlight;
    if let Err(err) = run(config, cli, text).await {
        let highlight = stderr().is_terminal() && highlight;
        render_error(err, highlight);
        std::process::exit(1);
    }
    Ok(())
}

async fn run(config: GlobalConfig, cli: Cli, text: Option<String>) -> Result<()> {
    let abort_signal = create_abort_signal();

    if let Some(addr) = cli.serve {
        return serve::run(config, addr).await;
    }
    if cli.list_models {
        for model in list_chat_models(&config.read()) {
            println!("{}", model.id());
        }
        return Ok(());
    }
    if cli.list_roles {
        let roles = Config::list_roles(true).join("\n");
        println!("{roles}");
        return Ok(());
    }
    if cli.list_agents {
        let agents = list_agents().join("\n");
        println!("{agents}");
        return Ok(());
    }
    if cli.list_rags {
        let rags = Config::list_rags().join("\n");
        println!("{rags}");
        return Ok(());
    }
    if cli.dry_run {
        config.write().dry_run = true;
    }

    if let Some(agent) = &cli.agent {
        let session = cli.session.as_ref().map(|v| match v {
            Some(v) => v.as_str(),
            None => TEMP_SESSION_NAME,
        });
        Config::use_agent(&config, agent, session, abort_signal.clone()).await?
    } else {
        if let Some(prompt) = &cli.prompt {
            config.write().use_prompt(prompt)?;
        } else if let Some(name) = &cli.role {
            config.write().use_role(name)?;
        } else if cli.execute {
            config.write().use_role(SHELL_ROLE)?;
        } else if cli.code {
            config.write().use_role(CODE_ROLE)?;
        }
        if let Some(session) = &cli.session {
            config
                .write()
                .use_session(session.as_ref().map(|v| v.as_str()))?;
        }
        if let Some(rag) = &cli.rag {
            Config::use_rag(&config, Some(rag), abort_signal.clone()).await?;
        }
    }
    if cli.list_sessions {
        let sessions = config.read().list_sessions().join("\n");
        println!("{sessions}");
        return Ok(());
    }
    if let Some(model_id) = &cli.model {
        config.write().set_model(model_id)?;
    }
    if cli.no_stream {
        config.write().stream = false;
    }
    if cli.save_session {
        config.write().set_save_session(Some(true));
    }
    if cli.info {
        let info = config.read().info()?;
        println!("{}", info);
        return Ok(());
    }
    let is_repl = config.read().working_mode.is_repl();
    if cli.execute && !is_repl {
        let input = create_input(&config, text, &cli.file).await?;
        shell_execute(&config, &SHELL, input).await?;
        return Ok(());
    }
    config.write().apply_prelude()?;
    match is_repl {
        false => {
            let mut input = create_input(&config, text, &cli.file).await?;
            input.use_embeddings(abort_signal.clone()).await?;
            start_directive(&config, input, cli.code, abort_signal).await
        }
        true => start_interactive(&config).await,
    }
}

#[async_recursion::async_recursion]
async fn start_directive(
    config: &GlobalConfig,
    input: Input,
    code_mode: bool,
    abort_signal: AbortSignal,
) -> Result<()> {
    let client = input.create_client()?;
    let extract_code = !*IS_STDOUT_TERMINAL && code_mode;
    config.write().before_chat_completion(&input)?;
    let (output, tool_results) = if !config.read().stream || extract_code {
        let task = client.chat_completions(input.clone());
        let ret = run_with_spinner(task, "Generating").await;
        match ret {
            Ok(ret) => {
                let ChatCompletionsOutput {
                    mut text,
                    tool_calls,
                    ..
                } = ret;
                if !text.is_empty() {
                    if extract_code && text.trim_start().starts_with("```") {
                        text = extract_block(&text);
                    }
                    config.read().print_markdown(&text)?;
                }
                (text, eval_tool_calls(config, tool_calls)?)
            }
            Err(err) => return Err(err),
        }
    } else {
        call_chat_completions_streaming(&input, client.as_ref(), config, abort_signal.clone())
            .await?
    };
    config
        .write()
        .after_chat_completion(&input, &output, &tool_results)?;

    if need_send_tool_results(&tool_results) {
        start_directive(
            config,
            input.merge_tool_call(output, tool_results),
            code_mode,
            abort_signal,
        )
        .await?;
    }

    config.write().exit_session()?;
    Ok(())
}

async fn start_interactive(config: &GlobalConfig) -> Result<()> {
    let mut repl: Repl = Repl::init(config)?;
    repl.run().await
}

#[async_recursion::async_recursion]
async fn shell_execute(config: &GlobalConfig, shell: &Shell, mut input: Input) -> Result<()> {
    let client = input.create_client()?;
    config.write().before_chat_completion(&input)?;
    let ret = if *IS_STDOUT_TERMINAL {
        let spinner = create_spinner("Generating").await;
        let ret = client.chat_completions(input.clone()).await;
        spinner.stop();
        ret
    } else {
        client.chat_completions(input.clone()).await
    };
    let mut eval_str = ret?.text;
    if let Ok(true) = CODE_BLOCK_RE.is_match(&eval_str) {
        eval_str = extract_block(&eval_str);
    }
    config
        .write()
        .after_chat_completion(&input, &eval_str, &[])?;
    if eval_str.is_empty() {
        bail!("No command generated");
    }
    if config.read().dry_run {
        config.read().print_markdown(&eval_str)?;
        return Ok(());
    }
    if *IS_STDOUT_TERMINAL {
        let options = ["execute", "revise", "describe", "cancel"];
        let command = color_text(eval_str.trim(), nu_ansi_term::Color::Rgb(255, 165, 0));
        let first_letter_color = nu_ansi_term::Color::Cyan;
        let prompt_text = options
            .iter()
            .map(|v| format!("{}{}", color_text(&v[0..1], first_letter_color), &v[1..]))
            .collect::<Vec<String>>()
            .join(&dimmed_text(" | "));
        loop {
            println!("{command}");
            let answer = Text::new(&format!("{prompt_text}:"))
                .with_default("e")
                .with_validator(|input: &str| match matches!(input, "e" | "r" | "d" | "c") {
                    true => Ok(Validation::Valid),
                    false => Ok(Validation::Invalid(
                        "Invalid option, choice one of e, r, d or c".into(),
                    )),
                })
                .prompt()?;

            match answer.as_str() {
                "e" => {
                    debug!("{} {:?}", shell.cmd, &[&shell.arg, &eval_str]);
                    let code = run_command(&shell.cmd, &[&shell.arg, &eval_str], None)?;
                    if code != 0 {
                        process::exit(code);
                    }
                }
                "r" => {
                    let revision = Text::new("Enter your revision:").prompt()?;
                    let text = format!("{}\n{revision}", input.text());
                    input.set_text(text);
                    return shell_execute(config, shell, input).await;
                }
                "d" => {
                    let role = config.read().retrieve_role(EXPLAIN_SHELL_ROLE)?;
                    let input = Input::from_str(config, &eval_str, Some(role));
                    let abort = create_abort_signal();
                    if config.read().stream {
                        call_chat_completions_streaming(&input, client.as_ref(), config, abort)
                            .await?;
                    } else {
                        call_chat_completions(&input, client.as_ref(), config).await?;
                    }
                    println!();
                    continue;
                }
                _ => {}
            }
            break;
        }
    } else {
        println!("{}", eval_str);
    }
    Ok(())
}

fn aggregate_text(text: Option<String>) -> Result<Option<String>> {
    let text = if stdin().is_terminal() {
        text
    } else {
        let mut stdin_text = String::new();
        stdin().read_to_string(&mut stdin_text)?;
        if let Some(text) = text {
            Some(format!("{text}\n{stdin_text}"))
        } else {
            Some(stdin_text)
        }
    };
    Ok(text)
}

async fn create_input(
    config: &GlobalConfig,
    text: Option<String>,
    file: &[String],
) -> Result<Input> {
    let input = if file.is_empty() {
        Input::from_str(config, &text.unwrap_or_default(), None)
    } else {
        Input::from_files(config, &text.unwrap_or_default(), file.to_vec(), None).await?
    };
    if input.is_empty() {
        bail!("No input");
    }
    Ok(input)
}

fn setup_logger(is_serve: bool) -> Result<()> {
    let (log_level, log_path) = Config::log_config(is_serve)?;
    if log_level == LevelFilter::Off {
        return Ok(());
    }
    let crate_name = env!("CARGO_CRATE_NAME");
    let log_filter = match std::env::var(get_env_name("log_filter")) {
        Ok(v) => v,
        Err(_) => match is_serve {
            true => format!("{crate_name}::serve"),
            false => crate_name.into(),
        },
    };
    let config = ConfigBuilder::new()
        .add_filter_allow(log_filter)
        .set_time_format_custom(format_description!(
            "[year]-[month]-[day]T[hour]:[minute]:[second].[subsecond digits:3]Z"
        ))
        .set_thread_level(LevelFilter::Off)
        .build();
    match log_path {
        None => {
            SimpleLogger::init(log_level, config)?;
        }
        Some(log_path) => {
            ensure_parent_exists(&log_path)?;
            let log_file = std::fs::File::create(log_path)?;
            WriteLogger::init(log_level, config, log_file)?;
        }
    }
    Ok(())
}
