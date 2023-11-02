mod role;
mod session;

use self::role::Role;
use self::session::{Session, TEMP_SESSION_NAME};

use crate::client::{
    create_client_config, list_client_types, list_models, ClientConfig, ExtraConfig, Message,
    Model, OpenAIClient, SendData,
};
use crate::render::{MarkdownRender, RenderOptions};
use crate::utils::{get_env_name, light_theme_from_colorfgbg, now, prompt_op_err};

use anyhow::{anyhow, bail, Context, Result};
use inquire::{Confirm, Select, Text};
use is_terminal::IsTerminal;
use parking_lot::RwLock;
use serde::Deserialize;
use std::{
    env,
    fs::{create_dir_all, read_dir, read_to_string, remove_file, File, OpenOptions},
    io::{stdout, Write},
    path::{Path, PathBuf},
    process::exit,
    sync::Arc,
};
use syntect::highlighting::ThemeSet;

/// Monokai Extended
const DARK_THEME: &[u8] = include_bytes!("../../assets/monokai-extended.theme.bin");
const LIGHT_THEME: &[u8] = include_bytes!("../../assets/monokai-extended-light.theme.bin");

const CONFIG_FILE_NAME: &str = "config.yaml";
const ROLES_FILE_NAME: &str = "roles.yaml";
const MESSAGES_FILE_NAME: &str = "messages.md";
const SESSIONS_DIR_NAME: &str = "sessions";

const CLIENTS_FIELD: &str = "clients";

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct Config {
    /// LLM model
    #[serde(rename(serialize = "model", deserialize = "model"))]
    pub model_id: Option<String>,
    /// GPT temperature, between 0 and 2
    #[serde(rename(serialize = "temperature", deserialize = "temperature"))]
    pub default_temperature: Option<f64>,
    /// Whether to save the message
    pub save: bool,
    /// Whether to disable highlight
    pub highlight: bool,
    /// Dry-run flag
    pub dry_run: bool,
    /// Whether to use a light theme
    pub light_theme: bool,
    /// Specify the text-wrapping mode (no, auto, <max-width>)
    pub wrap: Option<String>,
    /// Whether wrap code block
    pub wrap_code: bool,
    /// Automatically copy the last output to the clipboard
    pub auto_copy: bool,
    /// REPL keybindings. values: emacs, vi
    pub keybindings: Keybindings,
    /// Setup AIs
    pub clients: Vec<ClientConfig>,
    /// Predefined roles
    #[serde(skip)]
    pub roles: Vec<Role>,
    /// Current selected role
    #[serde(skip)]
    pub role: Option<Role>,
    /// Current session
    #[serde(skip)]
    pub session: Option<Session>,
    #[serde(skip)]
    pub model: Model,
    #[serde(skip)]
    pub last_message: Option<(String, String)>,
    #[serde(skip)]
    pub temperature: Option<f64>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            model_id: None,
            default_temperature: None,
            save: true,
            highlight: true,
            dry_run: false,
            light_theme: false,
            wrap: None,
            wrap_code: false,
            auto_copy: false,
            keybindings: Default::default(),
            clients: vec![ClientConfig::default()],
            roles: vec![],
            role: None,
            session: None,
            model: Default::default(),
            last_message: None,
            temperature: None,
        }
    }
}

pub type GlobalConfig = Arc<RwLock<Config>>;

impl Config {
    pub fn init(is_interactive: bool) -> Result<Self> {
        let config_path = Self::config_file()?;

        let api_key = env::var("OPENAI_API_KEY").ok();

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

        if let Some(wrap) = config.wrap.clone() {
            config.set_wrap(&wrap)?;
        }

        config.temperature = config.default_temperature;

        config.load_roles()?;

        config.setup_model()?;
        config.setup_highlight();
        config.setup_light_theme()?;

        setup_logger()?;

        Ok(config)
    }

    pub fn retrieve_role(&self, name: &str) -> Result<Role> {
        self.roles
            .iter()
            .find(|v| v.match_name(name))
            .map(|v| {
                let mut role = v.clone();
                role.complete_prompt_args(name);
                role
            })
            .ok_or_else(|| anyhow!("Unknown role `{name}`"))
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

    pub fn local_path(name: &str) -> Result<PathBuf> {
        let mut path = Self::config_dir()?;
        path.push(name);
        Ok(path)
    }

    pub fn save_message(&mut self, input: &str, output: &str) -> Result<()> {
        self.last_message = Some((input.to_string(), output.to_string()));

        if self.dry_run {
            return Ok(());
        }

        if let Some(session) = self.session.as_mut() {
            session.add_message(input, output)?;
            return Ok(());
        }

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
        Self::local_path(CONFIG_FILE_NAME)
    }

    pub fn roles_file() -> Result<PathBuf> {
        let env_name = get_env_name("roles_file");
        env::var(env_name).map_or_else(
            |_| Self::local_path(ROLES_FILE_NAME),
            |value| Ok(PathBuf::from(value)),
        )
    }

    pub fn messages_file() -> Result<PathBuf> {
        Self::local_path(MESSAGES_FILE_NAME)
    }

    pub fn sessions_dir() -> Result<PathBuf> {
        Self::local_path(SESSIONS_DIR_NAME)
    }

    pub fn session_file(name: &str) -> Result<PathBuf> {
        let mut path = Self::sessions_dir()?;
        path.push(&format!("{name}.yaml"));
        Ok(path)
    }

    pub fn set_role(&mut self, name: &str) -> Result<()> {
        let role = self.retrieve_role(name)?;
        if let Some(session) = self.session.as_mut() {
            session.update_role(Some(role.clone()))?;
        }
        self.temperature = role.temperature;
        self.role = Some(role);
        Ok(())
    }

    pub fn clear_role(&mut self) -> Result<()> {
        if let Some(session) = self.session.as_mut() {
            session.update_role(None)?;
        }
        self.temperature = self.default_temperature;
        self.role = None;
        Ok(())
    }

    pub fn get_temperature(&self) -> Option<f64> {
        self.temperature
    }

    pub fn set_temperature(&mut self, value: Option<f64>) -> Result<()> {
        self.temperature = value;
        if let Some(session) = self.session.as_mut() {
            session.set_temperature(value);
        }
        Ok(())
    }

    pub fn echo_messages(&self, content: &str) -> String {
        if let Some(session) = self.session.as_ref() {
            session.echo_messages(content)
        } else if let Some(role) = self.role.as_ref() {
            role.echo_messages(content)
        } else {
            content.to_string()
        }
    }

    pub fn build_messages(&self, content: &str) -> Result<Vec<Message>> {
        let messages = if let Some(session) = self.session.as_ref() {
            session.build_emssages(content)
        } else if let Some(role) = self.role.as_ref() {
            role.build_messages(content)
        } else {
            let message = Message::new(content);
            vec![message]
        };
        Ok(messages)
    }

    pub fn set_wrap(&mut self, value: &str) -> Result<()> {
        if value == "no" {
            self.wrap = None;
        } else if value == "auto" {
            self.wrap = Some(value.into());
        } else {
            value
                .parse::<u16>()
                .map_err(|_| anyhow!("Invalid wrap value"))?;
            self.wrap = Some(value.into())
        }
        Ok(())
    }

    pub fn set_model(&mut self, value: &str) -> Result<()> {
        let models = list_models(self);
        let mut model = None;
        let value = value.trim_end_matches(':');
        if value.contains(':') {
            if let Some(found) = models.iter().find(|v| v.id() == value) {
                model = Some(found.clone());
            }
        } else if let Some(found) = models.iter().find(|v| v.client_name == value) {
            model = Some(found.clone());
        }
        match model {
            None => bail!("Unknown model '{}'", value),
            Some(model) => {
                if let Some(session) = self.session.as_mut() {
                    session.set_model(model.clone())?;
                }
                self.model = model;
                Ok(())
            }
        }
    }

    pub fn sys_info(&self) -> Result<String> {
        let path_info = |path: &Path| {
            let state = if path.exists() { "" } else { " ⚠️" };
            format!("{}{state}", path.display())
        };
        let temperature = self
            .temperature
            .map_or_else(|| String::from("-"), |v| v.to_string());
        let wrap = self
            .wrap
            .clone()
            .map_or_else(|| String::from("no"), |v| v.to_string());
        let items = vec![
            ("model", self.model.id()),
            ("temperature", temperature),
            ("dry_run", self.dry_run.to_string()),
            ("save", self.save.to_string()),
            ("highlight", self.highlight.to_string()),
            ("wrap", wrap),
            ("wrap_code", self.wrap_code.to_string()),
            ("light_theme", self.light_theme.to_string()),
            ("keybindings", self.keybindings.stringify().into()),
            ("config_file", path_info(&Self::config_file()?)),
            ("roles_file", path_info(&Self::roles_file()?)),
            ("messages_file", path_info(&Self::messages_file()?)),
            ("sessions_dir", path_info(&Self::sessions_dir()?)),
        ];
        let output = items
            .iter()
            .map(|(name, value)| format!("{name:<20}{value}"))
            .collect::<Vec<String>>()
            .join("\n");
        Ok(output)
    }

    pub fn role_info(&self) -> Result<String> {
        if let Some(role) = &self.role {
            role.info()
        } else {
            bail!("No role")
        }
    }

    pub fn session_info(&self) -> Result<String> {
        if let Some(session) = &self.session {
            let render_options = self.get_render_options()?;
            let mut markdown_render = MarkdownRender::init(render_options)?;
            session.render(&mut markdown_render)
        } else {
            bail!("No session")
        }
    }

    pub fn info(&self) -> Result<String> {
        if let Some(session) = &self.session {
            session.export()
        } else if let Some(role) = &self.role {
            role.info()
        } else {
            self.sys_info()
        }
    }

    pub fn last_reply(&self) -> &str {
        self.last_message
            .as_ref()
            .map(|(_, reply)| reply.as_str())
            .unwrap_or_default()
    }

    pub fn repl_complete(&self, cmd: &str, args: &str) -> Vec<String> {
        let possible_values = match cmd {
            ".role" => self.roles.iter().map(|v| v.name.clone()).collect(),
            ".model" => list_models(self).into_iter().map(|v| v.id()).collect(),
            ".session" => self.list_sessions(),
            ".set" => {
                vec![
                    "temperature ".into(),
                    format!("save {}", !self.save),
                    format!("highlight {}", !self.highlight),
                    format!("dry_run {}", !self.dry_run),
                    format!("auto_copy {}", !self.auto_copy),
                ]
            }
            _ => vec![],
        };
        let mut possible_values: Vec<String> = possible_values
            .into_iter()
            .filter(|v| v.starts_with(args))
            .collect();
        possible_values.sort_unstable();
        possible_values
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
                let value = if unset {
                    None
                } else {
                    let value = value.parse().with_context(|| "Invalid value")?;
                    Some(value)
                };
                self.set_temperature(value)?;
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
            "auto_copy" => {
                let value = value.parse().with_context(|| "Invalid value")?;
                self.auto_copy = value;
            }
            _ => bail!("Unknown key `{key}`"),
        }
        Ok(())
    }

    pub fn start_session(&mut self, session: Option<&str>) -> Result<()> {
        if self.session.is_some() {
            bail!("Already in a session, please use '.clear session' to exit the session first?");
        }
        match session {
            None => {
                let session_file = Self::session_file(TEMP_SESSION_NAME)?;
                if session_file.exists() {
                    remove_file(session_file)
                        .with_context(|| "Failed to clean previous session")?;
                }
                self.session = Some(Session::new(
                    TEMP_SESSION_NAME,
                    self.model.clone(),
                    self.role.clone(),
                ));
            }
            Some(name) => {
                let session_path = Self::session_file(name)?;
                if !session_path.exists() {
                    self.session = Some(Session::new(name, self.model.clone(), self.role.clone()));
                } else {
                    let session = Session::load(name, &session_path)?;
                    let model = session.model().to_string();
                    self.temperature = session.temperature();
                    self.session = Some(session);
                    self.set_model(&model)?;
                }
            }
        }
        if let Some(session) = self.session.as_mut() {
            if session.is_empty() {
                if let Some((input, output)) = &self.last_message {
                    let ans = Confirm::new(
                        "Start a session that incorporates the last question and answer?",
                    )
                    .with_default(false)
                    .prompt()
                    .map_err(prompt_op_err)?;
                    if ans {
                        session.add_message(input, output)?;
                    }
                }
            }
        }
        Ok(())
    }

    pub fn end_session(&mut self) -> Result<()> {
        if let Some(mut session) = self.session.take() {
            self.last_message = None;
            self.temperature = self.default_temperature;
            if session.should_save() {
                let ans = Confirm::new("Save session?")
                    .with_default(false)
                    .prompt()
                    .map_err(prompt_op_err)?;
                if !ans {
                    return Ok(());
                }
                let mut name = session.name().to_string();
                if session.is_temp() {
                    name = Text::new("Session name:")
                        .with_default(&name)
                        .prompt()
                        .map_err(prompt_op_err)?;
                }
                let session_path = Self::session_file(&name)?;
                let sessions_dir = session_path.parent().ok_or_else(|| {
                    anyhow!("Unable to save session file to {}", session_path.display())
                })?;
                if !sessions_dir.exists() {
                    create_dir_all(sessions_dir).with_context(|| {
                        format!("Failed to create session_dir '{}'", sessions_dir.display())
                    })?;
                }
                session.save(&session_path)?;
            }
        }
        Ok(())
    }

    pub fn list_sessions(&self) -> Vec<String> {
        let sessions_dir = match Self::sessions_dir() {
            Ok(dir) => dir,
            Err(_) => return vec![],
        };
        match read_dir(&sessions_dir) {
            Ok(rd) => {
                let mut names = vec![];
                for entry in rd.flatten() {
                    let name = entry.file_name();
                    if let Some(name) = name.to_string_lossy().strip_suffix(".yaml") {
                        names.push(name.to_string());
                    }
                }
                names
            }
            Err(_) => vec![],
        }
    }

    pub fn get_render_options(&self) -> Result<RenderOptions> {
        let theme = if self.highlight {
            let theme_mode = if self.light_theme { "light" } else { "dark" };
            let theme_filename = format!("{theme_mode}.tmTheme");
            let theme_path = Self::local_path(&theme_filename)?;
            if theme_path.exists() {
                let theme = ThemeSet::get_theme(&theme_path)
                    .with_context(|| format!("Invalid theme at {}", theme_path.display()))?;
                Some(theme)
            } else {
                let theme = if self.light_theme {
                    bincode::deserialize_from(LIGHT_THEME).expect("Invalid builtin light theme")
                } else {
                    bincode::deserialize_from(DARK_THEME).expect("Invalid builtin dark theme")
                };
                Some(theme)
            }
        } else {
            None
        };
        let wrap = if stdout().is_terminal() {
            self.wrap.clone()
        } else {
            None
        };
        Ok(RenderOptions::new(theme, wrap, self.wrap_code))
    }

    pub fn render_prompt_right(&self) -> String {
        if let Some(session) = &self.session {
            let (tokens, percent) = session.tokens_and_percent();
            let percent = if percent == 0.0 {
                String::new()
            } else {
                format!("({percent}%)")
            };
            format!("{tokens}{percent}")
        } else {
            String::new()
        }
    }

    pub fn prepare_send_data(&self, content: &str, stream: bool) -> Result<SendData> {
        let messages = self.build_messages(content)?;
        self.model.max_tokens_limit(&messages)?;
        Ok(SendData {
            messages,
            temperature: self.get_temperature(),
            stream,
        })
    }

    pub fn maybe_print_send_tokens(&self, input: &str) {
        if self.dry_run {
            if let Ok(messages) = self.build_messages(input) {
                let tokens = self.model.total_tokens(&messages);
                println!(">>> This message consumes {tokens} tokens. <<<");
            }
        }
    }

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
        let ctx = || format!("Failed to load config at {}", config_path.display());
        let content = read_to_string(config_path).with_context(ctx)?;

        let config: Self = serde_yaml::from_str(&content)
            .map_err(|err| {
                let err_msg = err.to_string();
                if err_msg.starts_with(&format!("{}: ", CLIENTS_FIELD)) {
                    anyhow!("clients: invalid value")
                } else {
                    anyhow!("{err_msg}")
                }
            })
            .with_context(ctx)?;

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

    fn setup_model(&mut self) -> Result<()> {
        let model = match &self.model_id {
            Some(v) => v.clone(),
            None => {
                let models = list_models(self);
                if models.is_empty() {
                    bail!("No available model");
                }

                models[0].id()
            }
        };
        self.set_model(&model)?;
        Ok(())
    }

    fn setup_highlight(&mut self) {
        if let Ok(value) = env::var("NO_COLOR") {
            let mut no_color = false;
            set_bool(&mut no_color, &value);
            if no_color {
                self.highlight = false;
            }
        }
    }

    fn setup_light_theme(&mut self) -> Result<()> {
        if self.light_theme {
            return Ok(());
        }
        if let Ok(value) = env::var(get_env_name("light_theme")) {
            set_bool(&mut self.light_theme, &value);
            return Ok(());
        } else if let Ok(value) = env::var("COLORFGBG") {
            if let Some(light) = light_theme_from_colorfgbg(&value) {
                self.light_theme = light
            }
        };
        Ok(())
    }

    fn compat_old_config(&mut self, config_path: &PathBuf) -> Result<()> {
        let content = read_to_string(config_path)?;
        let value: serde_json::Value = serde_yaml::from_str(&content)?;
        if value.get(CLIENTS_FIELD).is_some() {
            return Ok(());
        }

        if let Some(model_name) = value.get("model").and_then(|v| v.as_str()) {
            if model_name.starts_with("gpt") {
                self.model_id = Some(format!("{}:{}", OpenAIClient::NAME, model_name));
            }
        }

        if let Some(ClientConfig::OpenAI(client_config)) = self.clients.get_mut(0) {
            if let Some(api_key) = value.get("api_key").and_then(|v| v.as_str()) {
                client_config.api_key = Some(api_key.to_string())
            }

            if let Some(organization_id) = value.get("organization_id").and_then(|v| v.as_str()) {
                client_config.organization_id = Some(organization_id.to_string())
            }

            let mut extra_config = ExtraConfig::default();

            if let Some(proxy) = value.get("proxy").and_then(|v| v.as_str()) {
                extra_config.proxy = Some(proxy.to_string())
            }

            if let Some(connect_timeout) = value.get("connect_timeout").and_then(|v| v.as_i64()) {
                extra_config.connect_timeout = Some(connect_timeout as _)
            }

            client_config.extra = Some(extra_config);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Deserialize, Default)]
pub enum Keybindings {
    #[serde(rename = "emacs")]
    #[default]
    Emacs,
    #[serde(rename = "vi")]
    Vi,
}

impl Keybindings {
    pub fn is_vi(&self) -> bool {
        matches!(self, Keybindings::Vi)
    }
    pub fn stringify(&self) -> &str {
        match self {
            Keybindings::Emacs => "emacs",
            Keybindings::Vi => "vi",
        }
    }
}

fn create_config_file(config_path: &Path) -> Result<()> {
    let ans = Confirm::new("No config file, create a new one?")
        .with_default(true)
        .prompt()
        .map_err(prompt_op_err)?;
    if !ans {
        exit(0);
    }

    let client = Select::new("Platform:", list_client_types())
        .prompt()
        .map_err(prompt_op_err)?;

    let mut config = serde_json::json!({});
    config["model"] = client.into();
    config[CLIENTS_FIELD] = create_client_config(client)?;

    let config_data = serde_yaml::to_string(&config).with_context(|| "Failed to create config")?;

    ensure_parent_exists(config_path)?;
    std::fs::write(config_path, config_data).with_context(|| "Failed to write to config file")?;
    #[cfg(unix)]
    {
        use std::os::unix::prelude::PermissionsExt;
        let perms = std::fs::Permissions::from_mode(0o600);
        std::fs::set_permissions(config_path, perms)?;
    }

    println!("✨ Saved config file to {}\n", config_path.display());

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

#[cfg(debug_assertions)]
fn setup_logger() -> Result<()> {
    use simplelog::WriteLogger;
    let file = std::fs::File::create(Config::local_path("debug.log")?)?;
    let config = simplelog::ConfigBuilder::new()
        .add_filter_allow_str("aichat")
        .build();
    WriteLogger::init(log::LevelFilter::Debug, config, file)?;
    Ok(())
}

#[cfg(not(debug_assertions))]
fn setup_logger() -> Result<()> {
    Ok(())
}
