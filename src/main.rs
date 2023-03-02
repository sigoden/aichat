mod config;

use std::io::{stdout, Write};
use std::path::Path;
use std::path::PathBuf;
use std::process::exit;

use config::Config;

use anyhow::{anyhow, Result};
use clap::{Arg, ArgAction, Command};
use eventsource_stream::{EventStream, Eventsource};
use futures_util::Stream;
use futures_util::StreamExt;
use inquire::{Confirm, Text};
use reedline::{
    default_emacs_keybindings, ColumnarMenu, DefaultCompleter, DefaultPrompt, DefaultPromptSegment,
    Emacs, FileBackedHistory, KeyCode, KeyModifiers, Reedline, ReedlineEvent, ReedlineMenu, Signal,
};
use reqwest::{Client, Proxy};
use serde_json::{json, Value};
use tokio::runtime::Runtime;

const API_URL: &str = "https://api.openai.com/v1/chat/completions";
const MODEL: &str = "gpt-3.5-turbo";
const REPL_COMMANDS: [(&str, &str); 6] = [
    (".clear", "Clear the screen"),
    (".clear-history", "Clear the history"),
    (".exit", " Exit the REPL"),
    (".help", "Print this help message"),
    (".history", "Print the history"),
    (".role", "Specify the role that the AI will play"),
];

fn main() {
    if let Err(err) = start() {
        eprintln!("error: {err}");
        exit(1);
    }
}

fn start() -> Result<()> {
    let matches = Command::new(env!("CARGO_CRATE_NAME"))
        .version(env!("CARGO_PKG_VERSION"))
        .author(env!("CARGO_PKG_AUTHORS"))
        .about(concat!(
            env!("CARGO_PKG_DESCRIPTION"),
            " - ",
            env!("CARGO_PKG_REPOSITORY")
        ))
        .arg(
            Arg::new("list-roles")
                .short('L')
                .long("list-roles")
                .action(ArgAction::SetTrue)
                .help("List all roles"),
        )
        .arg(
            Arg::new("role")
                .short('r')
                .long("role")
                .action(ArgAction::Set)
                .help("Specify the role that the AI will play"),
        )
        .arg(
            Arg::new("text")
                .action(ArgAction::Append)
                .help("Input text"),
        )
        .get_matches();
    let mut text = matches.get_many::<String>("text").map(|v| {
        v.map(|x| x.trim().to_string())
            .collect::<Vec<String>>()
            .join(" ")
    });
    let config_path = get_config_path()?;
    if !config_path.exists() && text.is_none() {
        create_config_file(&config_path)?;
    }
    let config = Config::init(&config_path)?;

    let role = matches.get_one::<String>("role").cloned();
    if let (Some(name), Some(text_)) = (role.as_ref(), text.as_ref()) {
        let role = config
            .roles
            .iter()
            .find(|v| &v.name == name)
            .ok_or_else(|| anyhow!("Unknown role \"{name}\" "))?;
        text = Some(role.generate(text_));
    };

    if matches.get_flag("list-roles") {
        config.roles.iter().for_each(|v| println!("{}", v.name));
        exit(1);
    }

    let client = init_client(&config)?;
    let runtime = init_runtime()?;
    match text {
        Some(text) => {
            let output = runtime.block_on(async move { acquire(&client, &config, &text).await })?;
            println!("{output}");
        }
        None => run_repl(runtime, client, config, role)?,
    }

    Ok(())
}

fn run_repl(runtime: Runtime, client: Client, config: Config, role: Option<String>) -> Result<()> {
    print_repl_title();
    let mut commands: Vec<String> = REPL_COMMANDS
        .into_iter()
        .map(|(v, _)| v.to_string())
        .collect();
    commands.extend(config.roles.iter().map(|v| format!(".role {}", v.name)));
    let mut completer = DefaultCompleter::with_inclusions(&['.', '-']).set_min_word_len(2);
    completer.insert(commands.clone());
    let completer = Box::new(completer);
    let completion_menu = Box::new(ColumnarMenu::default().with_name("completion_menu"));
    let mut keybindings = default_emacs_keybindings();
    keybindings.add_binding(
        KeyModifiers::NONE,
        KeyCode::Tab,
        ReedlineEvent::UntilFound(vec![
            ReedlineEvent::Menu("completion_menu".to_string()),
            ReedlineEvent::MenuNext,
        ]),
    );
    let history = Box::new(
        FileBackedHistory::with_file(1000, get_history_path()?)
            .map_err(|err| anyhow!("Failed to setup history file, {err}"))?,
    );
    let edit_mode = Box::new(Emacs::new(keybindings));
    let mut line_editor = Reedline::create()
        .with_completer(completer)
        .with_history(history)
        .with_menu(ReedlineMenu::EngineCompleter(completion_menu))
        .with_edit_mode(edit_mode);
    let prompt = DefaultPrompt::new(DefaultPromptSegment::Empty, DefaultPromptSegment::Empty);
    let mut trigged_ctrlc = false;
    let send_line = |line: String, trigged_ctrlc: &mut bool| -> Result<()> {
        *trigged_ctrlc = false;
        if line.is_empty() {
            return Ok(());
        }
        runtime.block_on(async {
            tokio::select! {
                ret = handle_input(&client, &config, &line) => {
                    if let Err(err) = ret {
                        dump(format!("error: {err}"));
                    }
                }
                _ =  tokio::signal::ctrl_c() => {
                    *trigged_ctrlc = true;
                    dump(" Abort current session")
                }
            }
        });
        Ok(())
    };

    let handle_line =
        |line_editor: &mut Reedline, line: String, trigged_ctrlc: &mut bool| -> Result<bool> {
            if line.starts_with('.') {
                let (name, args) = match line.split_once(' ') {
                    Some((head, tail)) => (head, Some(tail.trim())),
                    None => (line.as_str(), None),
                };
                match name {
                    ".exit" => {
                        return Ok(true);
                    }
                    ".help" => {
                        dump(get_repl_help());
                    }
                    ".clear" => {
                        line_editor.clear_scrollback()?;
                    }
                    ".clear-history" => {
                        let history = Box::new(line_editor.history_mut());
                        history
                            .clear()
                            .map_err(|err| anyhow!("Failed to clear history, {err}"))?;
                    }
                    ".history" => {
                        line_editor.print_history()?;
                    }
                    ".role" => match args {
                        Some(name) => match config.roles.iter().find(|v| v.name == name) {
                            Some(role) => {
                                send_line(role.prompt.clone(), trigged_ctrlc)?;
                            }
                            None => dump("Unknown role"),
                        },
                        None => dump("Usage: .role <name>"),
                    },
                    _ => {
                        dump("Unknown command. Type \".help\" for more information.");
                    }
                }
            } else {
                send_line(line, trigged_ctrlc)?;
            }
            Ok(false)
        };
    if let Some(name) = role {
        handle_line(&mut line_editor, format!("role {name}"), &mut trigged_ctrlc)?;
    }
    loop {
        let sig = line_editor.read_line(&prompt);
        match sig {
            Ok(Signal::Success(line)) => {
                let quit = handle_line(&mut line_editor, line, &mut trigged_ctrlc)?;
                if quit {
                    break;
                }
            }
            Ok(Signal::CtrlC) => {
                if !trigged_ctrlc {
                    trigged_ctrlc = true;
                    dump("(To exit, press Ctrl+C again or Ctrl+D or type .exit)");
                } else {
                    break;
                }
            }
            Ok(Signal::CtrlD) => {
                break;
            }
            Err(err) => {
                eprintln!("{err:?}");
                break;
            }
        }
    }
    Ok(())
}

async fn handle_input(client: &Client, config: &Config, text: &str) -> Result<()> {
    if config.dry_run {
        dump(text);
        return Ok(());
    }
    let mut stream = acquire_stream(client, config, text).await?;
    while let Some(part) = stream.next().await {
        let chunk = part?.data;
        if chunk == "[DONE]" {
            println!();
            stdout().flush().unwrap();
            break;
        } else {
            let data: Value = serde_json::from_str(&chunk)?;
            let text = data["choices"][0]["delta"]["content"]
                .as_str()
                .unwrap_or_default();

            print!("{text}");
            stdout().flush().unwrap();
        }
    }
    Ok(())
}

fn init_client(config: &Config) -> Result<Client> {
    let mut builder = Client::builder();
    if let Some(proxy) = config.proxy.as_ref() {
        builder =
            builder.proxy(Proxy::all(proxy).map_err(|err| anyhow!("Invalid config.proxy, {err}"))?);
    }
    let client = builder
        .build()
        .map_err(|err| anyhow!("Failed to init http client, {err}"))?;
    Ok(client)
}

fn init_runtime() -> Result<Runtime> {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|err| anyhow!("Failed to init tokio, {err}"))
}

fn create_config_file(config_path: &Path) -> Result<()> {
    let ans = Confirm::new("No config file, create a new one?")
        .with_default(true)
        .prompt()
        .map_err(|_| anyhow!("Error with questionnaire, try again later"))?;
    if !ans {
        exit(0);
    }
    let api_key = Text::new("Openai API Key:")
        .prompt()
        .map_err(|_| anyhow!("An error happened when asking for your key, try again later."))?;
    std::fs::write(config_path, format!("api_key = \"{api_key}\"\n"))
        .map_err(|err| anyhow!("Failed to write to config file, {err}"))?;
    Ok(())
}

async fn acquire(client: &Client, config: &Config, content: &str) -> Result<String> {
    if config.dry_run {
        return Ok(content.to_string());
    }
    let body = json!({
        "model": MODEL,
        "messages": [{"role": "user", "content": content}]
    });

    let data: Value = client
        .post(API_URL)
        .bearer_auth(&config.api_key)
        .json(&body)
        .send()
        .await?
        .json()
        .await?;

    let output = data["choices"][0]["message"]["content"]
        .as_str()
        .ok_or_else(|| anyhow!("Unexpect response {data}"))?;

    Ok(output.to_string())
}

async fn acquire_stream(
    client: &Client,
    config: &Config,
    content: &str,
) -> Result<EventStream<impl Stream<Item = reqwest::Result<bytes::Bytes>>>> {
    let body = json!({
        "model": MODEL,
        "messages": [{"role": "user", "content": content}],
        "stream": true,
    });

    let stream = client
        .post(API_URL)
        .bearer_auth(&config.api_key)
        .json(&body)
        .send()
        .await?
        .bytes_stream()
        .eventsource();

    Ok(stream)
}

fn dump<T: ToString>(text: T) {
    println!("{}", text.to_string());
    stdout().flush().unwrap();
}

fn get_config_path() -> Result<PathBuf> {
    let config_dir = dirs::home_dir().ok_or_else(|| anyhow!("No home dir"))?;
    Ok(config_dir.join(format!(".{}.toml", env!("CARGO_CRATE_NAME"))))
}

fn get_history_path() -> Result<PathBuf> {
    let config_dir = dirs::home_dir().ok_or_else(|| anyhow!("No home dir"))?;
    Ok(config_dir.join(format!(".{}_history", env!("CARGO_CRATE_NAME"))))
}

fn print_repl_title() {
    println!("Welcome to aichat {}", env!("CARGO_PKG_VERSION"));
    println!("Type \".help\" for more information.");
}

fn get_repl_help() -> String {
    let head = REPL_COMMANDS
        .iter()
        .map(|(name, desc)| format!("{name:<15} {desc}"))
        .collect::<Vec<String>>()
        .join("\n");
    format!("{head}\n\nPress Ctrl+C to abort session, Ctrl+D to exit the REPL")
}
