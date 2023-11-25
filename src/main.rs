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

use anyhow::Result;
use clap::Parser;
use client::{init_client, list_models};
use config::Input;
use is_terminal::IsTerminal;
use parking_lot::RwLock;
use render::{render_error, render_stream, MarkdownRender};
use repl::Repl;
use std::io::{stderr, stdin, stdout, Read};
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
    if let Some(name) = &cli.role {
        config.write().set_role(name)?;
    }
    if let Some(session) = &cli.session {
        config
            .write()
            .start_session(session.as_ref().map(|v| v.as_str()))?;
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
    config.write().onstart()?;
    if let Err(err) = start(&config, text, cli.file, cli.no_stream) {
        let highlight = stderr().is_terminal() && config.read().highlight;
        render_error(err, highlight)
    }
    Ok(())
}

fn start(
    config: &GlobalConfig,
    text: Option<String>,
    include: Option<Vec<String>>,
    no_stream: bool,
) -> Result<()> {
    if stdin().is_terminal() {
        match text {
            Some(text) => start_directive(config, &text, include, no_stream),
            None => start_interactive(config),
        }
    } else {
        let mut input = String::new();
        stdin().read_to_string(&mut input)?;
        if let Some(text) = text {
            input = format!("{text}\n{input}");
        }
        start_directive(config, &input, include, no_stream)
    }
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
    let client = init_client(config)?;
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
