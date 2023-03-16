mod cli;
mod client;
mod config;
mod render;
mod repl;
mod term;
#[macro_use]
mod utils;

use crate::cli::Cli;
use crate::client::ChatGptClient;
use crate::config::{Config, SharedConfig};

use anyhow::{anyhow, Result};
use clap::Parser;
use crossbeam::sync::WaitGroup;
use is_terminal::IsTerminal;
use parking_lot::RwLock;
use render::{render_stream, MarkdownRender};
use repl::{AbortSignal, Repl};
use std::io::{stdin, Read};
use std::sync::Arc;
use std::{io::stdout, process::exit};
use utils::cl100k_base_singleton;

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
        exit(0);
    }
    if cli.list_models {
        config::MODELS
            .iter()
            .for_each(|(name, _)| println!("{}", name));
        exit(0);
    }
    let role = match &cli.role {
        Some(name) => Some(
            config
                .read()
                .get_role(name)
                .ok_or_else(|| anyhow!("Unknown role '{name}'"))?,
        ),
        None => None,
    };
    if let Some(model) = &cli.model {
        config.write().set_model(model)?;
    }
    config.write().role = role;
    if cli.no_highlight {
        config.write().highlight = false;
    }
    if let Some(prompt) = &cli.prompt {
        config.write().add_prompt(prompt)?;
    }
    let no_stream = cli.no_stream;
    let client = ChatGptClient::init(config.clone())?;
    if atty::isnt(atty::Stream::Stdin) {
        let mut input = String::new();
        stdin().read_to_string(&mut input)?;
        if let Some(text) = text {
            input = format!("{text}\n{input}");
        }
        start_directive(client, config, &input, no_stream)
    } else {
        match text {
            Some(text) => start_directive(client, config, &text, no_stream),
            None => start_interactive(client, config),
        }
    }
}

fn start_directive(
    client: ChatGptClient,
    config: SharedConfig,
    input: &str,
    no_stream: bool,
) -> Result<()> {
    if !stdout().is_terminal() {
        config.write().highlight = false;
    }
    let output = if no_stream {
        let (highlight, light_theme) = config.read().get_render_options();
        let output = client.send_message(input)?;
        if highlight {
            let mut markdown_render = MarkdownRender::new(light_theme);
            println!("{}", markdown_render.render(&output).trim());
        } else {
            println!("{}", output.trim());
        }
        output
    } else {
        let wg = WaitGroup::new();
        let abort = AbortSignal::new();
        let abort_clone = abort.clone();
        ctrlc::set_handler(move || {
            abort_clone.set_ctrlc();
        })
        .expect("Error setting Ctrl-C handler");
        let output = render_stream(input, &client, config.clone(), false, abort, wg.clone())?;
        wg.wait();
        output
    };
    config.read().save_message(input, &output)
}

fn start_interactive(client: ChatGptClient, config: SharedConfig) -> Result<()> {
    cl100k_base_singleton();
    config.write().on_repl()?;
    let mut repl = Repl::init(config.clone())?;
    repl.run(client, config)
}
