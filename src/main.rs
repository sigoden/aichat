mod cli;
mod client;
mod config;
mod logger;
mod render;
mod repl;
mod serve;
#[macro_use]
mod utils;

#[macro_use]
extern crate log;

use crate::cli::Cli;
use crate::client::{ensure_model_capabilities, init_client, list_models, send_stream};
use crate::config::{
    Config, GlobalConfig, Input, InputContext, WorkingMode, CODE_ROLE, EXPLAIN_SHELL_ROLE,
    SHELL_ROLE,
};
use crate::render::{render_error, MarkdownRender};
use crate::repl::Repl;
use crate::utils::{
    cl100k_base_singleton, create_abort_signal, extract_block, run_command, run_spinner,
    CODE_BLOCK_RE,
};

use anyhow::{bail, Result};
use clap::Parser;
use inquire::{Select, Text};
use is_terminal::IsTerminal;
use parking_lot::RwLock;
use std::io::{stderr, stdin, stdout, Read};
use std::process;
use std::sync::Arc;
use tokio::sync::oneshot;

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let text = cli.text();
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

    if let Some(addr) = cli.serve {
        return serve::run(config, addr).await;
    }
    if cli.list_roles {
        config
            .read()
            .roles
            .iter()
            .for_each(|v| println!("{}", v.name));
        return Ok(());
    }
    if cli.list_models {
        for model in list_models(&config.read()) {
            println!("{}", model.id());
        }
        return Ok(());
    }
    if cli.list_sessions {
        let sessions = config.read().list_sessions().join("\n");
        println!("{sessions}");
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
    if let Some(prompt) = &cli.prompt {
        config.write().set_prompt(prompt)?;
    } else if let Some(name) = &cli.role {
        config.write().set_role(name)?;
    } else if cli.execute {
        config.write().set_role(SHELL_ROLE)?;
    } else if cli.code {
        config.write().set_role(CODE_ROLE)?;
    }
    if let Some(session) = &cli.session {
        config
            .write()
            .start_session(session.as_ref().map(|v| v.as_str()))?;
    }
    if let Some(model) = &cli.model {
        config.write().set_model(model)?;
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
    let text = aggregate_text(text)?;
    if cli.execute {
        if no_input {
            bail!("No input");
        }
        let input = create_input(&config, text, file)?;
        execute(&config, input).await?;
        return Ok(());
    }
    config.write().apply_prelude()?;
    if let Err(err) = match no_input {
        false => {
            let input = create_input(&config, text, file)?;
            start_directive(&config, input, cli.no_stream, cli.code).await
        }
        true => start_interactive(&config).await,
    } {
        let highlight = stderr().is_terminal() && config.read().highlight;
        render_error(err, highlight)
    }
    Ok(())
}

async fn start_directive(
    config: &GlobalConfig,
    input: Input,
    no_stream: bool,
    code_mode: bool,
) -> Result<()> {
    let mut client = init_client(config)?;
    ensure_model_capabilities(client.as_mut(), input.required_capabilities())?;
    config.read().maybe_print_send_tokens(&input);
    let is_terminal_stdout = stdout().is_terminal();
    let extract_code = !is_terminal_stdout && code_mode;
    let output = if no_stream || extract_code {
        let (output, _) = client.send_message(input.clone()).await?;
        let output = if extract_code && output.trim_start().starts_with("```") {
            extract_block(&output)
        } else {
            output.clone()
        };
        if is_terminal_stdout {
            let render_options = config.read().get_render_options()?;
            let mut markdown_render = MarkdownRender::init(render_options)?;
            println!("{}", markdown_render.render(&output).trim());
        } else {
            println!("{}", output);
        }
        output
    } else {
        let abort = create_abort_signal();
        send_stream(&input, client.as_ref(), config, abort).await?
    };
    // Save the message/session
    config.write().save_message(input, &output)?;
    config.write().end_session()?;
    Ok(())
}

async fn start_interactive(config: &GlobalConfig) -> Result<()> {
    cl100k_base_singleton();
    let mut repl: Repl = Repl::init(config)?;
    repl.run().await
}

#[async_recursion::async_recursion]
async fn execute(config: &GlobalConfig, mut input: Input) -> Result<()> {
    let client = init_client(config)?;
    config.read().maybe_print_send_tokens(&input);
    let is_terminal_stdout = stdout().is_terminal();
    let ret = if is_terminal_stdout {
        let (spinner_tx, spinner_rx) = oneshot::channel();
        tokio::spawn(run_spinner(" Generating", spinner_rx));
        let ret = client.send_message(input.clone()).await;
        let _ = spinner_tx.send(());
        ret
    } else {
        client.send_message(input.clone()).await
    };
    let (mut eval_str, _) = ret?;
    if let Ok(true) = CODE_BLOCK_RE.is_match(&eval_str) {
        eval_str = extract_block(&eval_str);
    }
    config.write().save_message(input.clone(), &eval_str)?;
    config.read().maybe_copy(&eval_str);
    let render_options = config.read().get_render_options()?;
    let mut markdown_render = MarkdownRender::init(render_options)?;
    if config.read().dry_run {
        println!("{}", markdown_render.render(&eval_str).trim());
        return Ok(());
    }
    if is_terminal_stdout {
        loop {
            let answer = Select::new(
                markdown_render.render(&eval_str).trim(),
                vec!["✅ Execute", "🤔 Revise", "📙 Explain", "❌ Cancel"],
            )
            .prompt()?;

            match answer {
                "✅ Execute" => {
                    let code = run_command(&eval_str)?;
                    if code != 0 {
                        process::exit(code);
                    }
                }
                "🤔 Revise" => {
                    let revision = Text::new("Enter your revision:").prompt()?;
                    let text = format!("{}\n{revision}", input.text());
                    input.set_text(text);
                    return execute(config, input).await;
                }
                "📙 Explain" => {
                    let role = config.read().retrieve_role(EXPLAIN_SHELL_ROLE)?;
                    let input = Input::from_str(&eval_str, InputContext::role(role));
                    let abort = create_abort_signal();
                    send_stream(&input, client.as_ref(), config, abort).await?;
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

fn create_input(config: &GlobalConfig, text: Option<String>, file: &[String]) -> Result<Input> {
    let input_context = config.read().input_context();
    let input = if file.is_empty() {
        Input::from_str(&text.unwrap_or_default(), input_context)
    } else {
        Input::new(&text.unwrap_or_default(), file.to_vec(), input_context)?
    };
    if input.is_empty() {
        bail!("No input");
    }
    Ok(input)
}
