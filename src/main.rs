mod cli;
mod client;
mod config;
mod render;
mod repl;
mod term;
#[macro_use]
mod utils;

use crate::cli::Cli;
use crate::client::Client;
use crate::config::{Config, SharedConfig};

use anyhow::{anyhow, Context, Result};
use clap::Parser;
use client::{init_client, list_models};
use crossbeam::sync::WaitGroup;
use is_terminal::IsTerminal;
use parking_lot::RwLock;
use render::{render_stream, MarkdownRender};
use repl::{AbortSignal, Repl};
use std::io::{stdin, Read};
use std::sync::Arc;
use std::{io::stdout, process::exit};
use tokio::runtime::Runtime;
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
        for model in list_models(&config.read()) {
            println!("{}", model.stringify());
        }
        exit(0);
    }
    if cli.dry_run {
        config.write().dry_run = true;
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
    if cli.info {
        let info = config.read().info()?;
        println!("{info}");
        exit(0);
    }
    let no_stream = cli.no_stream;
    let runtime = init_runtime()?;
    let client = init_client(config.clone(), runtime)?;
    if atty::isnt(atty::Stream::Stdin) {
        let mut input = String::new();
        stdin().read_to_string(&mut input)?;
        if let Some(text) = text {
            input = format!("{text}\n{input}");
        }
        start_directive(client.as_ref(), &config, &input, no_stream)
    } else {
        match text {
            Some(text) => start_directive(client.as_ref(), &config, &text, no_stream),
            None => start_interactive(client, config),
        }
    }
}

fn start_directive(
    client: &dyn Client,
    config: &SharedConfig,
    input: &str,
    no_stream: bool,
) -> Result<()> {
    if !stdout().is_terminal() {
        config.write().highlight = false;
    }
    config.read().maybe_print_send_tokens(input);
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
        let output = render_stream(input, client, config, false, abort, wg.clone())?;
        wg.wait();
        output
    };
    config.read().save_message(input, &output)
}

fn start_interactive(client: Box<dyn Client>, config: SharedConfig) -> Result<()> {
    cl100k_base_singleton();
    config.write().on_repl()?;
    let mut repl = Repl::init(config.clone())?;
    repl.run(client, config)
}

fn init_runtime() -> Result<Runtime> {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .with_context(|| "Failed to init tokio")
}
