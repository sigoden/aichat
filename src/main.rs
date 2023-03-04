mod cli;
mod client;
mod config;
mod editor;
mod render;
mod repl;

use std::sync::Arc;
use std::{io::stdout, process::exit};

use cli::Cli;
use client::ChatGptClient;
use config::{Config, Role};
use is_terminal::IsTerminal;

use anyhow::{anyhow, Result};
use clap::Parser;
use render::MarkdownRender;
use repl::{Repl, ReplCmdHandler};

fn main() {
    if let Err(err) = start() {
        eprintln!("error: {err}");
        exit(1);
    }
}

fn start() -> Result<()> {
    let cli = Cli::parse();
    let text = cli.text();
    let config = Arc::new(Config::init(text.is_none())?);
    if cli.list_roles {
        config.roles.iter().for_each(|v| println!("{}", v.name));
        exit(0);
    }
    let role = match &cli.role {
        Some(name) => Some(
            config
                .find_role(name)
                .ok_or_else(|| anyhow!("Unknown role '{name}'"))?,
        ),
        None => None,
    };
    let client = ChatGptClient::init(config.clone())?;
    match text {
        Some(text) => start_directive(client, config, role, &text),
        None => start_interactive(client, config, role),
    }
}

fn start_directive(
    client: ChatGptClient,
    config: Arc<Config>,
    role: Option<Role>,
    input: &str,
) -> Result<()> {
    let mut file = config.open_message_file()?;
    let output = client.acquire(input, role.map(|v| v.prompt))?;
    let output = output.trim();
    if !config.no_highlight && stdout().is_terminal() {
        let markdown_render = MarkdownRender::init()?;
        markdown_render.print(output)?;
    } else {
        println!("{output}");
    }

    Config::save_message(file.as_mut(), input, output);
    Ok(())
}

fn start_interactive(client: ChatGptClient, config: Arc<Config>, role: Option<Role>) -> Result<()> {
    let mut repl = Repl::init(config.clone())?;
    let handler = ReplCmdHandler::init(client, config, role)?;
    repl.run(handler)
}
