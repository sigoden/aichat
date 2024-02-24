mod cli;
mod client;
mod config;
mod render;
mod repl;

#[macro_use]
extern crate log;
#[macro_use]
mod utils;

use crate::cli::Cli;
use crate::config::{Config, GlobalConfig};
use crate::utils::run_command;

use anyhow::{bail, Result};
use clap::Parser;
use client::{ensure_model_capabilities, init_client, list_models};
use config::Input;
use inquire::validator::Validation;
use inquire::Text;
use is_terminal::IsTerminal;
use parking_lot::RwLock;
use render::{render_error, render_stream, MarkdownRender};
use repl::Repl;
use std::io::{stderr, stdin, stdout, Read};
use std::process;
use std::sync::Arc;
use utils::{cl100k_base_singleton, create_abort_signal};

fn main() -> Result<()> {
    let cli = Cli::parse();
    let text = cli.text();
    let config = Arc::new(RwLock::new(Config::init(text.is_none())?));
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
    if cli.execute {
        config.write().set_execute_role()?;
    } else {
        if let Some(name) = &cli.role {
            config.write().set_role(name)?;
        }
        if let Some(session) = &cli.session {
            config
                .write()
                .start_session(session.as_ref().map(|v| v.as_str()))?;
        }
    }
    if let Some(model) = &cli.model {
        config.write().set_model(model)?;
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
        match text {
            Some(text) => {
                execute(&config, &text)?;
                return Ok(());
            }
            None => bail!("No input text"),
        }
    }
    config.write().prelude()?;
    if let Err(err) = match text {
        Some(text) => start_directive(&config, &text, cli.file, cli.no_stream),
        None => start_interactive(&config),
    } {
        let highlight = stderr().is_terminal() && config.read().highlight;
        render_error(err, highlight)
    }
    Ok(())
}

fn start_directive(
    config: &GlobalConfig,
    text: &str,
    include: Option<Vec<String>>,
    no_stream: bool,
) -> Result<()> {
    if let Some(session) = &config.read().session {
        session.guard_save()?;
    }
    let input = Input::new(text, include.unwrap_or_default())?;
    let mut client = init_client(config)?;
    ensure_model_capabilities(client.as_mut(), input.required_capabilities())?;
    config.read().maybe_print_send_tokens(&input);
    let output = if no_stream {
        let output = client.send_message(input.clone())?;
        if stdout().is_terminal() {
            let render_options = config.read().get_render_options()?;
            let mut markdown_render = MarkdownRender::init(render_options)?;
            println!("{}", markdown_render.render(&output).trim());
        } else {
            println!("{}", output);
        }
        output
    } else {
        let abort = create_abort_signal();
        render_stream(&input, client.as_ref(), config, abort)?
    };
    config.write().save_message(input, &output)
}

fn start_interactive(config: &GlobalConfig) -> Result<()> {
    cl100k_base_singleton();
    let mut repl: Repl = Repl::init(config)?;
    repl.run()
}

fn execute(config: &GlobalConfig, text: &str) -> Result<()> {
    let input = Input::from_str(text);
    let client = init_client(config)?;
    config.read().maybe_print_send_tokens(&input);
    let eval_str = client.send_message(input.clone())?;
    let render_options = config.read().get_render_options()?;
    let mut markdown_render = MarkdownRender::init(render_options)?;
    if config.read().dry_run {
        println!("{}", markdown_render.render(&eval_str).trim());
        return Ok(());
    }
    if stdout().is_terminal() {
        println!("{}", markdown_render.render(&eval_str).trim());
        let mut describe = false;
        loop {
            let anwser = Text::new("[e]xecute, [d]escribe, [a]bort: ")
                .with_default("e")
                .with_validator(|input: &str| {
                    match matches!(input, "E" | "e" | "D" | "d" | "A" | "a") {
                        true => Ok(Validation::Valid),
                        false => Ok(Validation::Invalid(
                            "Invalid input, choice one of e, d or a".into(),
                        )),
                    }
                })
                .prompt()?;

            match anwser.as_str() {
                "E" | "e" => {
                    let code = run_command(&eval_str)?;
                    if code != 0 {
                        process::exit(code);
                    }
                }
                "D" | "d" => {
                    if !describe {
                        config.write().set_describe_role()?;
                    }
                    let input = Input::from_str(&eval_str);
                    let abort = create_abort_signal();
                    render_stream(&input, client.as_ref(), config, abort)?;
                    describe = true;
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
