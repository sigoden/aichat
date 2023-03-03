mod cli;
mod client;
mod config;
mod repl;

use std::process::exit;
use std::sync::Arc;

use cli::Cli;
use client::ChatGptClient;
use config::{Config, Role};

use anyhow::{anyhow, Result};
use clap::Parser;
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
                .ok_or_else(|| anyhow!("Uknown role '{name}'"))?,
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
    println!("{}", output.trim());
    Config::save_message(file.as_mut(), input, &output);
    Ok(())
}

fn start_interactive(client: ChatGptClient, config: Arc<Config>, role: Option<Role>) -> Result<()> {
    let mut repl = Repl::init(config.clone())?;
    let handler = ReplCmdHandler::init(client, config, role)?;
    repl.run(handler)
}
