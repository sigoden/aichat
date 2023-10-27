mod conversation;
mod message;
mod role;

use self::conversation::Conversation;
use self::message::Message;
use self::role::Role;

use crate::client::openai::{OpenAIClient, OpenAIConfig};
use crate::client::{all_clients, create_client_config, list_models, ClientConfig, ModelInfo};
use crate::config::message::num_tokens_from_messages;
use crate::utils::{get_env_name, now};

use anyhow::{anyhow, bail, Context, Result};
use inquire::{Confirm, Select};
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
const SET_COMPLETIONS: [&str; 7] = [
    ".set temperature",
    ".set save true",
    ".set save false",
    ".set highlight true",
    ".set highlight false",
    ".set dry_run true",
    ".set dry_run false",
];

#[allow(clippy::struct_excessive_bools)]
#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct Config {
    /// LLM model
    pub model: Option<String>,
    /// What sampling temperature to use, between 0 and 2
    pub temperature: Option<f64>,
    /// Whether to persistently save chat messages
    pub save: bool,
    /// Whether to disable highlight
    pub highlight: bool,
    /// Used only for debugging
    pub dry_run: bool,
    /// If set ture, start a conversation immediately upon repl
    pub conversation_first: bool,
    /// Is ligth theme
    pub light_theme: bool,
    /// Automatically copy the last output to the clipboard
    pub auto_copy: bool,
    /// Use vi keybindings, overriding the default Emacs keybindings
    pub vi_keybindings: bool,
    /// LLM clients
    pub clients: Vec<ClientConfig>,
    /// Predefined roles
    #[serde(skip)]
    pub roles: Vec<Role>,
    /// Current selected role
    #[serde(skip)]
    pub role: Option<Role>,
    /// Current conversation
    #[serde(skip)]
    pub conversation: Option<Conversation>,
    #[serde(skip)]
    pub model_info: ModelInfo,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            model: None,
            temperature: None,
            save: false,
            highlight: true,
            dry_run: false,
            conversation_first: false,
            light_theme: false,
            auto_copy: false,
            vi_keybindings: false,
            roles: vec![],
            clients: vec![ClientConfig::OpenAI(OpenAIConfig::default())],
            role: None,
            conversation: None,
            model_info: Default::default(),
        }
    }
}

#[allow(clippy::module_name_repetitions)]
pub type SharedConfig = Arc<RwLock<Config>>;

impl Config {
    pub fn init(is_interactive: bool) -> Result<Self> {
        let config_path = Self::config_file()?;

        let api_key = env::var(get_env_name("api_key")).ok();

        let exist_config_path = config_path.exists();
        if is_interactive && api_key.is_none() && !exist_config_path {
            create_config_file(&config_path)?;
        }
        let mut config = if api_key.is_some() && !exist_config_path {
            Self::default()
        } else {
            Self::load_config(&config_path)?
        };

        // Compatible with old configuration files
        if exist_config_path {
            config.compat_old_config(&config_path)?;
        }

        if let Some(name) = config.model.clone() {
            config.set_model(&name)?;
        }
        config.merge_env_vars();
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
        let path = if let Some(v) = env::var_os(env_name) {
            PathBuf::from(v)
        } else {
            let mut dir = dirs::config_dir().ok_or_else(|| anyhow!("Not found config dir"))?;
            dir.push(env!("CARGO_CRATE_NAME"));
            dir
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
            Some(v) if v.is_temp() => {
                format!(
                    "# CHAT:[{timestamp}]\n{}\n{input}\n--------\n{output}\n--------\n\n",
                    v.prompt
                )
            }
            Some(v) => {
                format!(
                    "# CHAT:[{timestamp}] ({})\n{input}\n--------\n{output}\n--------\n\n",
                    v.name,
                )
            }
        };
        file.write_all(output.as_bytes())
            .with_context(|| "Failed to save message")
    }

    pub fn config_file() -> Result<PathBuf> {
        Self::local_file(CONFIG_FILE_NAME)
    }

    pub fn roles_file() -> Result<PathBuf> {
        let env_name = get_env_name("roles_file");
        env::var(env_name).map_or_else(
            |_| Self::local_file(ROLES_FILE_NAME),
            |value| Ok(PathBuf::from(value)),
        )
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
                let output = serde_yaml::to_string(&role)
                    .unwrap_or_else(|_| "Unable to echo role details".into());
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
        #[allow(clippy::option_if_let_else)]
        if let Some(conversation) = self.conversation.as_ref() {
            conversation.echo_messages(content)
        } else if let Some(role) = self.role.as_ref() {
            role.echo_messages(content)
        } else {
            content.to_string()
        }
    }

    pub fn build_messages(&self, content: &str) -> Result<Vec<Message>> {
        #[allow(clippy::option_if_let_else)]
        let messages = if let Some(conversation) = self.conversation.as_ref() {
            conversation.build_emssages(content)
        } else if let Some(role) = self.role.as_ref() {
            role.build_messages(content)
        } else {
            let message = Message::new(content);
            vec![message]
        };
        let tokens = num_tokens_from_messages(&messages);
        if tokens >= self.model_info.max_tokens {
            bail!("Exceed max tokens limit")
        }

        Ok(messages)
    }

    pub fn set_model(&mut self, value: &str) -> Result<()> {
        let models = list_models(self);
        if value.contains(':') {
            if let Some(model) = models.iter().find(|v| v.stringify() == value) {
                self.model_info = model.clone();
                return Ok(());
            }
        } else if let Some(model) = models.iter().find(|v| v.client == value) {
            self.model_info = model.clone();
            return Ok(());
        }
        bail!("Invalid model")
    }

    pub const fn get_reamind_tokens(&self) -> usize {
        let mut tokens = self.model_info.max_tokens;
        if let Some(conversation) = self.conversation.as_ref() {
            tokens = tokens.saturating_sub(conversation.tokens);
        }
        tokens
    }

    pub fn info(&self) -> Result<String> {
        let file_info = |path: &Path| {
            let state = if path.exists() { "" } else { " ⚠️" };
            format!("{}{state}", path.display())
        };
        let temperature = self
            .temperature
            .map_or_else(|| String::from("-"), |v| v.to_string());
        let items = vec![
            ("config_file", file_info(&Self::config_file()?)),
            ("roles_file", file_info(&Self::roles_file()?)),
            ("messages_file", file_info(&Self::messages_file()?)),
            ("model", self.model_info.stringify()),
            ("temperature", temperature),
            ("save", self.save.to_string()),
            ("highlight", self.highlight.to_string()),
            ("conversation_first", self.conversation_first.to_string()),
            ("light_theme", self.light_theme.to_string()),
            ("dry_run", self.dry_run.to_string()),
            ("vi_keybindings", self.vi_keybindings.to_string()),
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

        completion.extend(SET_COMPLETIONS.map(std::string::ToString::to_string));
        completion.extend(
            list_models(self)
                .iter()
                .map(|v| format!(".model {}", v.stringify())),
        );
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
            "dry_run" => {
                let value = value.parse().with_context(|| "Invalid value")?;
                self.dry_run = value;
            }
            _ => bail!("Error: Unknown key `{key}`"),
        }
        Ok(())
    }

    pub fn start_conversation(&mut self) -> Result<()> {
        if self.conversation.is_some() && self.get_reamind_tokens() > 0 {
            let ans = Confirm::new("Already in a conversation, start a new one?")
                .with_default(true)
                .prompt()?;
            if !ans {
                return Ok(());
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

    pub const fn get_render_options(&self) -> (bool, bool) {
        (self.highlight, self.light_theme)
    }

    pub fn maybe_print_send_tokens(&self, input: &str) {
        if self.dry_run {
            if let Ok(messages) = self.build_messages(input) {
                let tokens = num_tokens_from_messages(&messages);
                println!(">>> This message consumes {tokens} tokens. <<<");
            }
        }
    }

    #[allow(clippy::unused_self)] // TODO: do we need to take self here? it's not used in the fn
    fn open_message_file(&self) -> Result<File> {
        let path = Self::messages_file()?;
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

        let config: Self = serde_yaml::from_str(&content)
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

    fn compat_old_config(&mut self, config_path: &PathBuf) -> Result<()> {
        let content = read_to_string(config_path)?;
        let value: serde_json::Value = serde_yaml::from_str(&content)?;
        if value.get("client").is_some() {
            return Ok(());
        }

        if let Some(model_name) = value.get("model").and_then(|v| v.as_str()) {
            if model_name.starts_with("gpt") {
                self.model = Some(format!("{}:{}", OpenAIClient::name(), model_name));
            }
        }

        if let Some(ClientConfig::OpenAI(client_config)) = self.clients.get_mut(0) {
            if let Some(api_key) = value.get("api_key").and_then(|v| v.as_str()) {
                client_config.api_key = Some(api_key.to_string())
            }

            if let Some(organization_id) = value.get("organization_id").and_then(|v| v.as_str()) {
                client_config.organization_id = Some(organization_id.to_string())
            }

            if let Some(proxy) = value.get("proxy").and_then(|v| v.as_str()) {
                client_config.proxy = Some(proxy.to_string())
            }

            if let Some(connect_timeout) = value.get("connect_timeout").and_then(|v| v.as_i64()) {
                client_config.connect_timeout = Some(connect_timeout as _)
            }
        }
        Ok(())
    }
}

fn create_config_file(config_path: &Path) -> Result<()> {
    let ans = Confirm::new("No config file, create a new one?")
        .with_default(true)
        .prompt()
        .map_err(|_| anyhow!("Not finish questionnaire, try again later."))?;
    if !ans {
        exit(0);
    }

    let client = Select::new("Choose bots?", all_clients())
        .prompt()
        .map_err(|_| anyhow!("An error happened when selecting bots, try again later."))?;

    let mut raw_config = create_client_config(client)?;

    raw_config.push_str(&format!("model: {client}\n"));

    let ans = Confirm::new("Save chat messages")
        .with_default(true)
        .prompt()
        .map_err(|_| anyhow!("Not finish questionnaire, try again later."))?;

    if ans {
        raw_config.push_str("save: true\n");
    }
    ensure_parent_exists(config_path)?;
    std::fs::write(config_path, raw_config).with_context(|| "Failed to write to config file")?;
    #[cfg(unix)]
    {
        use std::os::unix::prelude::PermissionsExt;
        let perms = std::fs::Permissions::from_mode(0o600);
        std::fs::set_permissions(config_path, perms)?;
    }
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

fn set_bool(target: &mut bool, value: &str) {
    match value {
        "1" | "true" => *target = true,
        "0" | "false" => *target = false,
        _ => {}
    }
}
