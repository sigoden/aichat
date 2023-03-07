mod cli;
mod client;
mod config;
mod render;
mod repl;
mod term;
mod utils;

use std::cell::RefCell;
use std::io::{stdin, Read};
use std::sync::Arc;
use std::{io::stdout, process::exit};

use cli::Cli;
use client::ChatGptClient;
use config::{Config, SharedConfig};
use is_terminal::IsTerminal;

use anyhow::{anyhow, Result};
use clap::Parser;
use render::MarkdownRender;
use repl::Repl;

fn main() -> Result<()> {
    let cli = Cli::parse();
    let text = cli.text();
    let config = Arc::new(RefCell::new(Config::init(text.is_none())?));
    if cli.list_roles {
        config
            .borrow()
            .roles
            .iter()
            .for_each(|v| println!("{}", v.name));
        exit(0);
    }
    let role = match &cli.role {
        Some(name) => Some(
            config
                .borrow()
                .find_role(name)
                .ok_or_else(|| anyhow!("Unknown role '{name}'"))?,
        ),
        None => None,
    };
    config.borrow_mut().role = role;
    if cli.no_highlight {
        config.borrow_mut().highlight = false;
    }
    let client = ChatGptClient::init(config.clone())?;
    if atty::isnt(atty::Stream::Stdin) {
        let mut input = String::new();
        stdin().read_to_string(&mut input)?;
        if let Some(text) = text {
            input = format!("{text}\n{input}");
        }
        start_directive(client, config, &input)
    } else {
        match text {
            Some(text) => start_directive(client, config, &text),
            None => start_interactive(client, config),
        }
    }
}

fn start_directive(client: ChatGptClient, config: SharedConfig, input: &str) -> Result<()> {
    let mut file = config.borrow().open_message_file()?;
    let prompt = config.borrow().get_prompt();
    let output = client.send_message(input, prompt)?;
    let output = output.trim();
    if config.borrow().highlight && stdout().is_terminal() {
        let mut markdown_render = MarkdownRender::new();
        println!("{}", markdown_render.render(output))
    } else {
        println!("{output}");
    }

    config.borrow().save_message(file.as_mut(), input, output)
}

fn start_interactive(client: ChatGptClient, config: SharedConfig) -> Result<()> {
    let mut repl = Repl::init(config.clone())?;
    repl.run(client, config)
}
