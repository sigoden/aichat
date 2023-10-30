mod cli;
mod client;
mod config;
mod render;
mod repl;
#[macro_use]
mod utils;

use crate::cli::Cli;
use crate::client::Client;
use crate::config::{Config, SharedConfig};

use anyhow::Result;
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
    if cli.list_sessions {
        let sessions = config.read().list_sessions()?.join("\n");
        println!("{sessions}");
        exit(0);
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
    if let Some(model) = &cli.model {
        config.write().set_model(model)?;
    }
    if let Some(name) = &cli.role {
        config.write().set_role(name)?;
    }
    if let Some(session) = &cli.session {
        config.write().start_session(session)?;
    }
    if cli.no_highlight {
        config.write().highlight = false;
    }
    if cli.info {
        let info = if let Some(session) = &config.read().session {
            session.info()?
        } else if let Some(role) = &config.read().role {
            role.info()?
        } else {
            config.read().info()?
        };
        println!("{info}");
        exit(0);
    }
    let no_stream = cli.no_stream;
    let client = init_client(config.clone())?;
    if stdin().is_terminal() {
        match text {
            Some(text) => start_directive(client.as_ref(), &config, &text, no_stream),
            None => start_interactive(config),
        }
    } else {
        let mut input = String::new();
        stdin().read_to_string(&mut input)?;
        if let Some(text) = text {
            input = format!("{text}\n{input}");
        }
        start_directive(client.as_ref(), &config, &input, no_stream)
    }
}

fn start_directive(
    client: &dyn Client,
    config: &SharedConfig,
    input: &str,
    no_stream: bool,
) -> Result<()> {
    if let Some(sesion) = &config.read().session {
        sesion.guard_save()?;
    }
    if !stdout().is_terminal() {
        config.write().highlight = false;
    }
    config.read().maybe_print_send_tokens(input);
    let output = if no_stream {
        let render_options = config.read().get_render_options();
        let output = client.send_message(input)?;
        let mut markdown_render = MarkdownRender::init(render_options)?;
        println!("{}", markdown_render.render(&output).trim());
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
    config.write().save_message(input, &output)
}

fn start_interactive(config: SharedConfig) -> Result<()> {
    cl100k_base_singleton();
    let mut repl: Repl = Repl::init(config.clone())?;
    repl.run(config)
}
