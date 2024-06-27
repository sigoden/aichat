mod cli;
mod client;
mod config;
mod function;
mod logger;
mod rag;
mod render;
mod repl;
mod serve;
#[macro_use]
mod utils;

#[macro_use]
extern crate log;

use crate::cli::Cli;
use crate::client::{chat_completion_streaming, list_chat_models, ChatCompletionsOutput};
use crate::config::{
    list_agents, Config, GlobalConfig, Input, WorkingMode, CODE_ROLE, EXPLAIN_SHELL_ROLE,
    SHELL_ROLE, TEMP_SESSION_NAME,
};
use crate::function::{eval_tool_calls, need_send_tool_results};
use crate::render::{render_error, MarkdownRender};
use crate::repl::Repl;
use crate::utils::{
    create_abort_signal, create_spinner, detect_shell, extract_block, run_command, AbortSignal,
    Shell, CODE_BLOCK_RE, IS_STDOUT_TERMINAL,
};

use anyhow::{bail, Result};
use async_recursion::async_recursion;
use clap::Parser;
use inquire::{Select, Text};
use is_terminal::IsTerminal;
use parking_lot::RwLock;
use std::io::{stderr, stdin, Read};
use std::process;
use std::sync::Arc;

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let text = cli.text();
    let text = aggregate_text(text)?;
    let file = &cli.file;
    let no_input = text.is_none() && file.is_empty();
    let working_mode = if cli.serve.is_some() {
        WorkingMode::Serve
    } else if no_input {
        WorkingMode::Repl
    } else {
        WorkingMode::Command
    };
    crate::logger::setup_logger(working_mode)?;
    let config = Arc::new(RwLock::new(Config::init(working_mode)?));

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
        config
            .read()
            .roles
            .iter()
            .for_each(|v| println!("{}", v.name()));
        return Ok(());
    }
    if cli.list_agents {
        let agents = list_agents().join("\n");
        println!("{agents}");
        return Ok(());
    }
    if cli.list_rags {
        let rags = config.read().list_rags().join("\n");
        println!("{rags}");
        return Ok(());
    }
    if let Some(wrap) = &cli.wrap {
        config.write().set_wrap(wrap)?;
    }
    if cli.light_theme {
        config.write().light_theme = true;
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
    if cli.save_session {
        config.write().set_save_session(Some(true));
    }
    if cli.no_highlight {
        config.write().highlight = false;
    }
    if cli.info {
        let info = config.read().info()?;
        println!("{}", info);
        return Ok(());
    }
    if cli.execute {
        if no_input {
            bail!("No input");
        }
        let input = create_input(&config, text, file).await?;
        let shell = detect_shell();
        shell_execute(&config, &shell, input).await?;
        return Ok(());
    }
    config.write().apply_prelude()?;
    if let Err(err) = match no_input {
        false => {
            let mut input = create_input(&config, text, file).await?;
            input.use_embeddings(abort_signal.clone()).await?;
            start_directive(&config, input, cli.no_stream, cli.code, abort_signal).await
        }
        true => start_interactive(&config).await,
    } {
        let highlight = stderr().is_terminal() && config.read().highlight;
        render_error(err, highlight);
        std::process::exit(1);
    }
    Ok(())
}

#[async_recursion]
async fn start_directive(
    config: &GlobalConfig,
    input: Input,
    no_stream: bool,
    code_mode: bool,
    abort_signal: AbortSignal,
) -> Result<()> {
    let client = input.create_client()?;
    let extract_code = !*IS_STDOUT_TERMINAL && code_mode;
    config.write().before_chat_completion(&input)?;
    let (output, tool_results) = if no_stream || extract_code {
        let ChatCompletionsOutput {
            text, tool_calls, ..
        } = client.chat_completions(input.clone()).await?;
        if !tool_calls.is_empty() {
            (String::new(), eval_tool_calls(config, tool_calls)?)
        } else {
            let text = if extract_code && text.trim_start().starts_with("```") {
                extract_block(&text)
            } else {
                text.clone()
            };
            if *IS_STDOUT_TERMINAL {
                let render_options = config.read().render_options()?;
                let mut markdown_render = MarkdownRender::init(render_options)?;
                println!("{}", markdown_render.render(&text).trim());
            } else {
                println!("{}", text);
            }
            (text, vec![])
        }
    } else {
        chat_completion_streaming(&input, client.as_ref(), config, abort_signal.clone()).await?
    };
    config
        .write()
        .after_chat_completion(&input, &output, &tool_results)?;

    config.write().exit_session()?;

    if need_send_tool_results(&tool_results) {
        start_directive(
            config,
            input.merge_tool_call(output, tool_results),
            no_stream,
            code_mode,
            abort_signal,
        )
        .await
    } else {
        Ok(())
    }
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
    let render_options = config.read().render_options()?;
    let mut markdown_render = MarkdownRender::init(render_options)?;
    if config.read().dry_run {
        println!("{}", markdown_render.render(&eval_str).trim());
        return Ok(());
    }
    if *IS_STDOUT_TERMINAL {
        loop {
            let answer = Select::new(
                eval_str.trim(),
                vec!["✅ Execute", "🔄️ Revise", "📖 Explain", "❌ Cancel"],
            )
            .prompt()?;

            match answer {
                "✅ Execute" => {
                    debug!("{} {:?}", shell.cmd, &[&shell.arg, &eval_str]);
                    let code = run_command(&shell.cmd, &[&shell.arg, &eval_str], None)?;
                    if code != 0 {
                        process::exit(code);
                    }
                }
                "🔄️ Revise" => {
                    let revision = Text::new("Enter your revision:").prompt()?;
                    let text = format!("{}\n{revision}", input.text());
                    input.set_text(text);
                    return shell_execute(config, shell, input).await;
                }
                "📖 Explain" => {
                    let role = config.read().retrieve_role(EXPLAIN_SHELL_ROLE)?;
                    let input = Input::from_str(config, &eval_str, Some(role));
                    let abort = create_abort_signal();
                    chat_completion_streaming(&input, client.as_ref(), config, abort).await?;
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
