mod conversation;
mod message;
mod role;

use self::message::Message;
use self::role::Role;
use self::{conversation::Conversation, message::within_max_tokens_limit};

use crate::utils::now;

use anyhow::{anyhow, bail, Context, Result};
use inquire::{Confirm, Text};
use parking_lot::RwLock;
use serde::Deserialize;
use std::{
    env,
    fs::{create_dir_all, read_to_string, File, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    process::exit,
    sync::Arc,
};

const CONFIG_FILE_NAME: &str = "config.yaml";
const ROLES_FILE_NAME: &str = "roles.yaml";
const HISTORY_FILE_NAME: &str = "history.txt";
const MESSAGE_FILE_NAME: &str = "messages.md";
const SET_COMPLETIONS: [&str; 9] = [
    ".set api_key",
    ".set temperature",
    ".set save true",
    ".set save false",
    ".set highlight true",
    ".set highlight false",
    ".set proxy",
    ".set dry_run true",
    ".set dry_run false",
];

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct Config {
    /// Openai api key
    pub api_key: Option<String>,
    /// What sampling temperature to use, between 0 and 2
    pub temperature: Option<f64>,
    /// Whether to persistently save chat messages
    pub save: bool,
    /// Whether to disable highlight
    pub highlight: bool,
    /// Set proxy
    pub proxy: Option<String>,
    /// Used only for debugging
    pub dry_run: bool,
    /// If set ture, start a conversation immediately upon repl
    pub conversation_first: bool,
    /// Is ligth theme
    pub light_theme: bool,
    /// Predefined roles
    #[serde(skip)]
    pub roles: Vec<Role>,
    /// Current selected role
    #[serde(skip)]
    pub role: Option<Role>,
    /// Current conversation
    #[serde(skip)]
    pub conversation: Option<Conversation>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            api_key: None,
            temperature: None,
            save: false,
            highlight: true,
            proxy: None,
            dry_run: false,
            conversation_first: false,
            light_theme: false,
            roles: vec![],
            role: None,
            conversation: None,
        }
    }
}

pub type SharedConfig = Arc<RwLock<Config>>;

impl Config {
    pub fn init(is_interactive: bool) -> Result<Self> {
        let api_key = env::var(get_env_name("api_key")).ok();
        let config_path = Self::config_file()?;
        if is_interactive && api_key.is_none() && !config_path.exists() {
            create_config_file(&config_path)?;
        }
        let mut config = if api_key.is_some() && !config_path.exists() {
            Default::default()
        } else {
            Self::load_config(&config_path)?
        };
        if api_key.is_some() {
            config.api_key = api_key;
        }
        if config.api_key.is_none() {
            bail!("api_key not set");
        }
        config.merge_env_vars();
        config.maybe_proxy();
        config.load_roles()?;

        Ok(config)
    }

    pub fn on_repl(&mut self) -> Result<()> {
        if self.conversation_first {
            self.start_conversation()?;
        }
        Ok(())
    }

    pub fn get_role(&self, name: &str) -> Option<Role> {
        self.roles.iter().find(|v| v.match_name(name)).map(|v| {
            let mut role = v.clone();
            role.complete_prompt_args(name);
            role
        })
    }

    pub fn config_dir() -> Result<PathBuf> {
        let env_name = get_env_name("config_dir");
        let path = match env::var_os(env_name) {
            Some(v) => PathBuf::from(v),
            None => {
                let mut dir = dirs::config_dir().ok_or_else(|| anyhow!("Not found config dir"))?;
                dir.push(env!("CARGO_CRATE_NAME"));
                dir
            }
        };
        Ok(path)
    }

    pub fn local_file(name: &str) -> Result<PathBuf> {
        let mut path = Self::config_dir()?;
        path.push(name);
        Ok(path)
    }

    pub fn save_message(&self, input: &str, output: &str) -> Result<()> {
        if !self.save {
            return Ok(());
        }
        let mut file = self.open_message_file()?;
        if output.is_empty() || !self.save {
            return Ok(());
        }
        let timestamp = now();
        let output = match self.role.as_ref() {
            None => {
                format!("# CHAT:[{timestamp}]\n{input}\n--------\n{output}\n--------\n\n",)
            }
            Some(v) => {
                if v.is_temp() {
                    format!(
                        "# CHAT:[{timestamp}]\n{}\n{input}\n--------\n{output}\n--------\n\n",
                        v.prompt
                    )
                } else {
                    format!(
                        "# CHAT:[{timestamp}] ({})\n{input}\n--------\n{output}\n--------\n\n",
                        v.name,
                    )
                }
            }
        };
        file.write_all(output.as_bytes())
            .with_context(|| "Failed to save message")
    }

    pub fn config_file() -> Result<PathBuf> {
        Self::local_file(CONFIG_FILE_NAME)
    }

    pub fn get_api_key(&self) -> &String {
        self.api_key.as_ref().expect("api_key not set")
    }

    pub fn roles_file() -> Result<PathBuf> {
        let env_name = get_env_name("roles_file");
        if let Ok(value) = env::var(env_name) {
            Ok(PathBuf::from(value))
        } else {
            Self::local_file(ROLES_FILE_NAME)
        }
    }

    pub fn history_file() -> Result<PathBuf> {
        Self::local_file(HISTORY_FILE_NAME)
    }

    pub fn messages_file() -> Result<PathBuf> {
        Self::local_file(MESSAGE_FILE_NAME)
    }

    pub fn change_role(&mut self, name: &str) -> Result<String> {
        match self.get_role(name) {
            Some(role) => {
                if let Some(conversation) = self.conversation.as_mut() {
                    conversation.update_role(&role)?;
                }
                let output =
                    serde_yaml::to_string(&role).unwrap_or("Unable to echo role details".into());
                self.role = Some(role);
                Ok(output)
            }
            None => bail!("Error: Unknown role"),
        }
    }

    pub fn clear_role(&mut self) -> Result<()> {
        if let Some(conversation) = self.conversation.as_ref() {
            conversation.can_clear_role()?;
        }
        self.role = None;
        Ok(())
    }

    pub fn add_prompt(&mut self, prompt: &str) -> Result<()> {
        let role = Role::new(prompt, self.temperature);
        if let Some(conversation) = self.conversation.as_mut() {
            conversation.update_role(&role)?;
        }
        self.role = Some(role);
        Ok(())
    }

    pub fn get_temperature(&self) -> Option<f64> {
        self.role
            .as_ref()
            .and_then(|v| v.temperature)
            .or(self.temperature)
    }

    pub fn echo_messages(&self, content: &str) -> String {
        if let Some(conversation) = self.conversation.as_ref() {
            conversation.echo_messages(content)
        } else if let Some(role) = self.role.as_ref() {
            role.echo_messages(content)
        } else {
            content.to_string()
        }
    }

    pub fn build_messages(&self, content: &str) -> Result<Vec<Message>> {
        let messages = if let Some(conversation) = self.conversation.as_ref() {
            conversation.build_emssages(content)
        } else if let Some(role) = self.role.as_ref() {
            role.build_emssages(content)
        } else {
            let message = Message::new(content);
            vec![message]
        };
        within_max_tokens_limit(&messages)?;

        Ok(messages)
    }

    pub fn info(&self) -> Result<String> {
        let file_info = |path: &Path| {
            let state = if path.exists() { "" } else { " ⚠️" };
            format!("{}{state}", path.display())
        };
        let proxy = self
            .proxy
            .as_ref()
            .map(|v| v.to_string())
            .unwrap_or("-".into());
        let temperature = self
            .temperature
            .map(|v| v.to_string())
            .unwrap_or("-".into());
        let items = vec![
            ("config_file", file_info(&Config::config_file()?)),
            ("roles_file", file_info(&Config::roles_file()?)),
            ("messages_file", file_info(&Config::messages_file()?)),
            ("api_key", self.get_api_key().to_string()),
            ("temperature", temperature),
            ("save", self.save.to_string()),
            ("highlight", self.highlight.to_string()),
            ("proxy", proxy),
            ("conversation_first", self.conversation_first.to_string()),
            ("light_theme", self.light_theme.to_string()),
            ("dry_run", self.dry_run.to_string()),
        ];
        let mut output = String::new();
        for (name, value) in items {
            output.push_str(&format!("{name:<20}{value}\n"));
        }
        Ok(output)
    }

    pub fn repl_completions(&self) -> Vec<String> {
        let mut completion: Vec<String> = self
            .roles
            .iter()
            .map(|v| format!(".role {}", v.name))
            .collect();

        completion.extend(SET_COMPLETIONS.map(|v| v.to_string()));
        completion
    }

    pub fn update(&mut self, data: &str) -> Result<()> {
        let parts: Vec<&str> = data.split_whitespace().collect();
        if parts.len() != 2 {
            bail!("Usage: .set <key> <value>. If value is null, unset key.");
        }
        let key = parts[0];
        let value = parts[1];
        let unset = value == "null";
        match key {
            "api_key" => {
                if unset {
                    bail!("Error: Not allowed");
                } else {
                    self.api_key = Some(value.to_string());
                }
            }
            "temperature" => {
                if unset {
                    self.temperature = None;
                } else {
                    let value = value.parse().with_context(|| "Invalid value")?;
                    self.temperature = Some(value);
                }
            }
            "save" => {
                let value = value.parse().with_context(|| "Invalid value")?;
                self.save = value;
            }
            "highlight" => {
                let value = value.parse().with_context(|| "Invalid value")?;
                self.highlight = value;
            }
            "proxy" => {
                if unset {
                    self.proxy = None;
                } else {
                    self.proxy = Some(value.to_string());
                }
            }
            "dry_run" => {
                let value = value.parse().with_context(|| "Invalid value")?;
                self.dry_run = value;
            }
            _ => bail!("Error: Unknown key `{key}`"),
        }
        Ok(())
    }

    pub fn start_conversation(&mut self) -> Result<()> {
        if let Some(conversation) = self.conversation.as_ref() {
            if conversation.reamind_tokens() > 0 {
                let ans = Confirm::new("Already in a conversation, start a new one?")
                    .with_default(true)
                    .prompt()?;
                if !ans {
                    return Ok(());
                }
            }
        }
        self.conversation = Some(Conversation::new(self.role.clone()));
        Ok(())
    }

    pub fn end_conversation(&mut self) {
        self.conversation = None;
    }

    pub fn save_conversation(&mut self, input: &str, output: &str) -> Result<()> {
        if let Some(conversation) = self.conversation.as_mut() {
            conversation.add_message(input, output)?;
        }
        Ok(())
    }

    pub fn get_render_options(&self) -> (bool, bool) {
        (self.highlight, self.light_theme)
    }

    fn open_message_file(&self) -> Result<File> {
        let path = Config::messages_file()?;
        ensure_parent_exists(&path)?;
        OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .with_context(|| format!("Failed to create/append {}", path.display()))
    }

    fn load_config(config_path: &Path) -> Result<Self> {
        let content = read_to_string(config_path)
            .with_context(|| format!("Failed to load config at {}", config_path.display()))?;

        let config: Config = serde_yaml::from_str(&content)
            .with_context(|| format!("Invalid config at {}", config_path.display()))?;
        Ok(config)
    }

    fn load_roles(&mut self) -> Result<()> {
        let path = Self::roles_file()?;
        if !path.exists() {
            return Ok(());
        }
        let content = read_to_string(&path)
            .with_context(|| format!("Failed to load roles at {}", path.display()))?;
        let roles: Vec<Role> =
            serde_yaml::from_str(&content).with_context(|| "Invalid roles config")?;
        self.roles = roles;
        Ok(())
    }

    fn merge_env_vars(&mut self) {
        if let Ok(value) = env::var(get_env_name("dry_run")) {
            set_bool(&mut self.dry_run, &value);
        }
        if let Ok(value) = env::var(get_env_name("light_theme")) {
            set_bool(&mut self.light_theme, &value);
        }
        if let Ok(value) = env::var("NO_COLOR") {
            let mut no_color = false;
            set_bool(&mut no_color, &value);
            if no_color {
                self.highlight = false;
            }
        }
    }

    fn maybe_proxy(&mut self) {
        if self.proxy.is_some() {
            return;
        }
        if let Ok(value) = env::var("HTTPS_PROXY").or_else(|_| env::var("ALL_PROXY")) {
            self.proxy = Some(value);
        }
    }
}

fn create_config_file(config_path: &Path) -> Result<()> {
    let confirm_map_err = |_| anyhow!("Not finish questionnaire, try again later.");
    let text_map_err = |_| anyhow!("An error happened when asking for your key, try again later.");
    let ans = Confirm::new("No config file, create a new one?")
        .with_default(true)
        .prompt()
        .map_err(confirm_map_err)?;
    if !ans {
        exit(0);
    }
    let api_key = Text::new("Openai API Key:")
        .prompt()
        .map_err(text_map_err)?;
    let mut raw_config = format!("api_key: {api_key}\n");

    let ans = Confirm::new("Use proxy?")
        .with_default(false)
        .prompt()
        .map_err(confirm_map_err)?;
    if ans {
        let proxy = Text::new("Set proxy:").prompt().map_err(text_map_err)?;
        raw_config.push_str(&format!("proxy: {proxy}\n"));
    }

    let ans = Confirm::new("Save chat messages")
        .with_default(true)
        .prompt()
        .map_err(confirm_map_err)?;
    if ans {
        raw_config.push_str("save: true\n");
    }
    ensure_parent_exists(config_path)?;
    std::fs::write(config_path, raw_config).with_context(|| "Failed to write to config file")?;
    Ok(())
}

fn ensure_parent_exists(path: &Path) -> Result<()> {
    if path.exists() {
        return Ok(());
    }
    let parent = path
        .parent()
        .ok_or_else(|| anyhow!("Failed to write to {}, No parent path", path.display()))?;
    if !parent.exists() {
        create_dir_all(parent).with_context(|| {
            format!(
                "Failed to write {}, Cannot create parent directory",
                path.display()
            )
        })?;
    }
    Ok(())
}

fn get_env_name(key: &str) -> String {
    format!(
        "{}_{}",
        env!("CARGO_CRATE_NAME").to_ascii_uppercase(),
        key.to_ascii_uppercase(),
    )
}

fn set_bool(target: &mut bool, value: &str) {
    match value {
        "1" | "true" => *target = true,
        "0" | "false" => *target = false,
        _ => {}
    }
}
