mod config;

use std::fs::{File, OpenOptions};
use std::io::{stdout, Write};
use std::path::Path;
use std::process::exit;
use std::time::Duration;

use config::{Config, Role, CONFIG_FILE_NAME, HISTORY_FILE_NAME, MESSAGE_FILE_NAME};

use anyhow::{anyhow, Result};
use clap::{Arg, ArgAction, Command};
use eventsource_stream::{EventStream, Eventsource};
use futures_util::Stream;
use futures_util::StreamExt;
use inquire::{Confirm, Editor, Text};
use reedline::{
    default_emacs_keybindings, ColumnarMenu, DefaultCompleter, DefaultPrompt, DefaultPromptSegment,
    Emacs, FileBackedHistory, KeyCode, KeyModifiers, Reedline, ReedlineEvent, ReedlineMenu, Signal,
};
use reqwest::{Client, Proxy};
use serde_json::{json, Value};
use tokio::runtime::Runtime;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const API_URL: &str = "https://api.openai.com/v1/chat/completions";
const MODEL: &str = "gpt-3.5-turbo";
const REPL_COMMANDS: [(&str, &str); 7] = [
    (".clear", "Clear the screen"),
    (".clear-history", "Clear the history"),
    (".exit", "Exit the REPL"),
    (".help", "Print this help message"),
    (".history", "Print the history"),
    (".role", "Specify the role that the AI will play"),
    (".view", "Use an external editor to view the AI reply"),
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
    let config_path = Config::local_file(CONFIG_FILE_NAME)?;
    if !config_path.exists() && text.is_none() {
        create_config_file(&config_path)?;
    }
    let config = Config::init(&config_path)?;

    let role_name = matches.get_one::<String>("role").cloned();
    if let (Some(name), Some(text_)) = (role_name.as_ref(), text.as_ref()) {
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
            println!("{}", output.trim());
        }
        None => run_repl(runtime, client, config, role_name)?,
    }

    Ok(())
}

fn run_repl(
    runtime: Runtime,
    client: Client,
    config: Config,
    role_name: Option<String>,
) -> Result<()> {
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
        FileBackedHistory::with_file(1000, Config::local_file(HISTORY_FILE_NAME)?)
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
    let mut output = String::new();
    let mut role: Option<Role> = None;
    let mut save_file: Option<File> = if config.save {
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(Config::local_file(MESSAGE_FILE_NAME)?)
            .map_err(|err| anyhow!("Failed to create/append save_file, {err}"))?;
        Some(file)
    } else {
        None
    };
    let handle_line = |line: String,
                       line_editor: &mut Reedline,
                       trigged_ctrlc: &mut bool,
                       role: &mut Option<Role>,
                       output: &mut String,
                       save_file: &mut Option<File>|
     -> Result<bool> {
        if line.starts_with('.') {
            let (name, args) = match line.split_once(' ') {
                Some((head, tail)) => (head, Some(tail.trim())),
                None => (line.as_str(), None),
            };
            match name {
                ".view" => {
                    if output.is_empty() {
                        return Ok(false);
                    }
                    let _ = Editor::new("view ai reply with an external editor")
                        .with_file_extension(".md")
                        .with_predefined_text(output)
                        .prompt()?;
                    dump("", 1);
                }
                ".exit" => {
                    return Ok(true);
                }
                ".help" => {
                    dump(get_repl_help(), 2);
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
                    dump("", 1);
                }
                ".role" => match args {
                    Some(name) => match config.roles.iter().find(|v| v.name == name) {
                        Some(role_) => {
                            *role = Some(role_.clone());
                            dump("", 1);
                        }
                        None => dump("Unknown role.", 2),
                    },
                    None => dump("Usage: .role <name>.", 2),
                },
                _ => {
                    dump("Unknown command. Type \".help\" for more information.", 2);
                }
            }
        } else {
            let input = if let Some(role) = role.take() {
                role.generate(&line)
            } else {
                line
            };
            output.clear();
            *trigged_ctrlc = false;
            if input.is_empty() {
                return Ok(false);
            }
            runtime.block_on(async {
                tokio::select! {
                    ret = handle_input(&client, &config, &input, output) => {
                        if let Err(err) = ret {
                            dump(format!("error: {err}"), 2);
                        }
                    }
                    _ =  tokio::signal::ctrl_c() => {
                        *trigged_ctrlc = true;
                        dump(" Abort current session.", 2)
                    }
                }
            });
            if !output.is_empty() {
                if let Some(file) = save_file.as_mut() {
                    let _ = file.write_all(
                        format!("AICHAT: {input}\n\n--------\n{output}\n--------\n\n").as_bytes(),
                    );
                }
            }
        }
        Ok(false)
    };
    if let Some(name) = role_name {
        handle_line(
            format!(".role {name}"),
            &mut line_editor,
            &mut trigged_ctrlc,
            &mut role,
            &mut output,
            &mut save_file,
        )?;
    }
    loop {
        let sig = line_editor.read_line(&prompt);
        match sig {
            Ok(Signal::Success(line)) => {
                let quit = handle_line(
                    line,
                    &mut line_editor,
                    &mut trigged_ctrlc,
                    &mut role,
                    &mut output,
                    &mut save_file,
                )?;
                if quit {
                    break;
                }
            }
            Ok(Signal::CtrlC) => {
                if !trigged_ctrlc {
                    trigged_ctrlc = true;
                    dump("(To exit, press Ctrl+C again or Ctrl+D or type .exit)", 2);
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

async fn handle_input(
    client: &Client,
    config: &Config,
    input: &str,
    output: &mut String,
) -> Result<()> {
    if config.dry_run {
        output.push_str(input);
        dump(input, 2);
        return Ok(());
    }
    let mut stream = acquire_stream(client, config, input).await?;
    let mut virgin = true;
    while let Some(part) = stream.next().await {
        let chunk = part?.data;
        if chunk == "[DONE]" {
            output.push('\n');
            dump("", 2);
            break;
        } else {
            let data: Value = serde_json::from_str(&chunk)?;
            let text = data["choices"][0]["delta"]["content"]
                .as_str()
                .unwrap_or_default();
            if text.is_empty() {
                continue;
            }
            if virgin {
                virgin = false;
                if text == "\n\n" {
                    continue;
                }
            }
            output.push_str(text);
            dump(text, 0);
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
        .connect_timeout(CONNECT_TIMEOUT)
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
    let mut body = json!({
        "model": MODEL,
        "messages": [{"role": "user", "content": content}]
    });

    if let Some(v) = config.temperature {
        body.as_object_mut()
            .and_then(|m| m.insert("temperature".into(), json!(v)));
    }

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
        .ok_or_else(|| anyhow!("Unexpected response {data}"))?;

    Ok(output.to_string())
}

async fn acquire_stream(
    client: &Client,
    config: &Config,
    content: &str,
) -> Result<EventStream<impl Stream<Item = reqwest::Result<bytes::Bytes>>>> {
    let mut body = json!({
        "model": MODEL,
        "messages": [{"role": "user", "content": content}],
        "stream": true,
    });

    if let Some(v) = config.temperature {
        body.as_object_mut()
            .and_then(|m| m.insert("temperature".into(), json!(v)));
    }

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

fn dump<T: ToString>(text: T, newlines: usize) {
    print!("{}{}", text.to_string(), "\n".repeat(newlines));
    stdout().flush().unwrap();
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
