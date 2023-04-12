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
use is_terminal::IsTerminal;
use render::{render_stream, MarkdownRender};
use repl::{AbortSignal, Repl};
use std::io::{stdin, Read};
use std::sync::Arc;
use std::{io::stdout, process::exit};
use tokio::sync::Barrier;
use utils::cl100k_base_singleton;

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let mut config = Config::init_shared(cli.is_interactive())?;
    update_config_with_cli_options(&mut config, &cli)?;

    match (cli.list_roles, cli.list_models, cli.info) {
        (true, _, _) => list_roles_and_exit(&config),
        (_, true, _) => list_models_and_exit(),
        (_, _, true) => print_info_and_exit(&config),
        _ => (),
    }

    let no_stream = cli.no_stream;
    let client = ChatGptClient::init(config.clone())?;

    let text = cli.text();

    if atty::isnt(atty::Stream::Stdin) {
        process_stdin_and_execute(client, config, text, no_stream).await?;
    } else {
        match text {
            Some(text) => start_directive(client, config, &text, no_stream).await?,
            None => start_interactive(client, config).await?,
        }
    }

    Ok(())
}

fn list_roles_and_exit(config: &SharedConfig) {
    config
        .read()
        .roles
        .iter()
        .for_each(|v| println!("{}", v.name));

    exit(0);
}

fn list_models_and_exit() {
    config::MODELS
        .iter()
        .for_each(|(name, _)| println!("{}", name));

    exit(0);
}

fn print_info_and_exit(config: &SharedConfig) {
    let info = config.read().info().unwrap();
    println!("{}", info);
    exit(0);
}

fn update_config_with_cli_options(config: &mut SharedConfig, cli: &Cli) -> Result<()> {
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

    config.write().role = role;

    if let Some(model) = &cli.model {
        config.write().set_model(model)?;
    }

    if cli.no_highlight {
        config.write().highlight = false;
    }

    if let Some(prompt) = &cli.prompt {
        config.write().add_prompt(prompt)?;
    }

    Ok(())
}

async fn process_stdin_and_execute(
    client: ChatGptClient,
    config: SharedConfig,
    text: Option<String>,
    no_stream: bool,
) -> Result<()> {
    let mut input = String::new();
    stdin().read_to_string(&mut input)?;

    if let Some(text) = text {
        input = format!("{text}\n{input}");
    }

    start_directive(client, config, &input, no_stream).await?;

    Ok(())
}

async fn start_directive(
    client: ChatGptClient,
    config: SharedConfig,
    input: &str,
    no_stream: bool,
) -> Result<()> {
    if !stdout().is_terminal() {
        config.write().highlight = false;
    }

    config.read().maybe_print_send_tokens(input);

    let output = if no_stream {
        let (highlight, light_theme) = config.read().get_render_options();
        let output = client.send_message(input).await?;

        if highlight {
            let mut markdown_render = MarkdownRender::new(light_theme);
            println!("{}", markdown_render.render(&output).trim());
        } else {
            println!("{}", output.trim());
        }

        output
    } else {
        let barrier = Arc::new(Barrier::new(2));
        let abort = AbortSignal::new();
        let abort_clone = abort.clone();

        ctrlc::set_handler(move || {
            abort_clone.set_ctrlc();
        })
        .expect("Error setting Ctrl-C handler");

        let output = render_stream(
            input,
            &client,
            config.clone(),
            false,
            abort,
            barrier.clone(),
        )
        .await?;

        barrier.wait().await;

        output
    };

    config.read().save_message(input, &output)
}

async fn start_interactive(client: ChatGptClient, config: SharedConfig) -> Result<()> {
    cl100k_base_singleton();

    config.write().on_repl()?;

    let mut repl = Repl::init(config.clone())?;
    repl.run(client, config).await
}
