mod agent;
mod input;
mod role;
mod session;

pub use self::agent::{complete_agent_variables, list_agents, Agent, AgentVariables};
pub use self::input::Input;
pub use self::role::{
    Role, RoleLike, CODE_ROLE, CREATE_TITLE_ROLE, EXPLAIN_SHELL_ROLE, SHELL_ROLE,
};
use self::session::Session;

use crate::client::{
    create_client_config, list_client_types, list_models, ClientConfig, MessageContentToolCalls,
    Model, ModelType, ProviderModels, OPENAI_COMPATIBLE_PROVIDERS,
};
use crate::function::{FunctionDeclaration, Functions, ToolResult};
use crate::rag::Rag;
use crate::render::{MarkdownRender, RenderOptions};
use crate::repl::{run_repl_command, split_args_text};
use crate::utils::*;

use anyhow::{anyhow, bail, Context, Result};
use indexmap::IndexMap;
use inquire::{list_option::ListOption, validator::Validation, Confirm, MultiSelect, Select, Text};
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use serde_json::json;
use simplelog::LevelFilter;
use std::collections::{HashMap, HashSet};
use std::{
    env,
    fs::{
        create_dir_all, read_dir, read_to_string, remove_dir_all, remove_file, File, OpenOptions,
    },
    io::Write,
    path::{Path, PathBuf},
    process,
    sync::{Arc, OnceLock},
};
use syntect::highlighting::ThemeSet;
use terminal_colorsaurus::{color_scheme, ColorScheme, QueryOptions};

pub const TEMP_ROLE_NAME: &str = "%%";
pub const TEMP_RAG_NAME: &str = "temp";
pub const TEMP_SESSION_NAME: &str = "temp";

/// Monokai Extended
const DARK_THEME: &[u8] = include_bytes!("../../assets/monokai-extended.theme.bin");
const LIGHT_THEME: &[u8] = include_bytes!("../../assets/monokai-extended-light.theme.bin");

const CONFIG_FILE_NAME: &str = "config.yaml";
const ROLES_DIR_NAME: &str = "roles";
const MACROS_DIR_NAME: &str = "macros";
const ENV_FILE_NAME: &str = ".env";
const MESSAGES_FILE_NAME: &str = "messages.md";
const SESSIONS_DIR_NAME: &str = "sessions";
const RAGS_DIR_NAME: &str = "rags";
const FUNCTIONS_DIR_NAME: &str = "functions";
const FUNCTIONS_FILE_NAME: &str = "functions.json";
const FUNCTIONS_BIN_DIR_NAME: &str = "bin";
const AGENTS_DIR_NAME: &str = "agents";

const CLIENTS_FIELD: &str = "clients";

const SERVE_ADDR: &str = "127.0.0.1:8000";

const SYNC_MODELS_URL: &str =
    "https://raw.githubusercontent.com/sigoden/aichat/refs/heads/main/models.yaml";

const SUMMARIZE_PROMPT: &str =
    "Summarize the discussion briefly in 200 words or less to use as a prompt for future context.";
const SUMMARY_PROMPT: &str = "This is a summary of the chat history as a recap: ";

const RAG_TEMPLATE: &str = r#"Answer the query based on the context while respecting the rules. (user query, some textual context and rules, all inside xml tags)

<context>
__CONTEXT__
</context>

<rules>
- If you don't know, just say so.
- If you are not sure, ask for clarification.
- Answer in the same language as the user query.
- If the context appears unreadable or of poor quality, tell the user then answer as best as you can.
- If the answer is not in the context but you think you know the answer, explain that to the user then answer with your own knowledge.
- Answer directly and without using xml tags.
</rules>

<user_query>
__INPUT__
</user_query>"#;

const LEFT_PROMPT: &str = "{color.green}{?session {?agent {agent}>}{session}{?role /}}{!session {?agent {agent}>}}{role}{?rag @{rag}}{color.cyan}{?session )}{!session >}{color.reset} ";
const RIGHT_PROMPT: &str = "{color.purple}{?session {?consume_tokens {consume_tokens}({consume_percent}%)}{!consume_tokens {consume_tokens}}}{color.reset}";

static EDITOR: OnceLock<Option<String>> = OnceLock::new();

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct Config {
    #[serde(rename(serialize = "model", deserialize = "model"))]
    #[serde(default)]
    pub model_id: String,
    pub temperature: Option<f64>,
    pub top_p: Option<f64>,

    pub dry_run: bool,
    pub stream: bool,
    pub save: bool,
    pub keybindings: String,
    pub editor: Option<String>,
    pub wrap: Option<String>,
    pub wrap_code: bool,

    pub function_calling: bool,
    pub mapping_tools: IndexMap<String, String>,
    pub use_tools: Option<String>,

    pub repl_prelude: Option<String>,
    pub cmd_prelude: Option<String>,
    pub agent_prelude: Option<String>,

    pub pre_command: Option<String>,
    pub post_command: Option<String>,

    pub save_session: Option<bool>,
    pub compress_threshold: usize,
    pub summarize_prompt: Option<String>,
    pub summary_prompt: Option<String>,

    pub rag_embedding_model: Option<String>,
    pub rag_reranker_model: Option<String>,
    pub rag_top_k: usize,
    pub rag_chunk_size: Option<usize>,
    pub rag_chunk_overlap: Option<usize>,
    pub rag_template: Option<String>,

    #[serde(default)]
    pub document_loaders: HashMap<String, String>,

    pub highlight: bool,
    pub theme: Option<String>,
    pub left_prompt: Option<String>,
    pub right_prompt: Option<String>,

    pub serve_addr: Option<String>,
    pub user_agent: Option<String>,
    pub save_shell_history: bool,
    pub sync_models_url: Option<String>,

    pub clients: Vec<ClientConfig>,

    #[serde(skip)]
    pub macro_flag: bool,
    #[serde(skip)]
    pub info_flag: bool,
    #[serde(skip)]
    pub agent_variables: Option<AgentVariables>,

    #[serde(skip)]
    pub model: Model,
    #[serde(skip)]
    pub functions: Functions,
    #[serde(skip)]
    pub working_mode: WorkingMode,
    #[serde(skip)]
    pub last_message: Option<LastMessage>,

    #[serde(skip)]
    pub role: Option<Role>,
    #[serde(skip)]
    pub session: Option<Session>,
    #[serde(skip)]
    pub rag: Option<Arc<Rag>>,
    #[serde(skip)]
    pub agent: Option<Agent>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            model_id: Default::default(),
            temperature: None,
            top_p: None,

            dry_run: false,
            stream: true,
            save: false,
            keybindings: "emacs".into(),
            editor: None,
            wrap: None,
            wrap_code: false,

            function_calling: true,
            mapping_tools: Default::default(),
            use_tools: None,

            repl_prelude: None,
            cmd_prelude: None,
            agent_prelude: None,

            pre_command: None,
            post_command: None,

            save_session: None,
            compress_threshold: 4000,
            summarize_prompt: None,
            summary_prompt: None,

            rag_embedding_model: None,
            rag_reranker_model: None,
            rag_top_k: 5,
            rag_chunk_size: None,
            rag_chunk_overlap: None,
            rag_template: None,

            document_loaders: Default::default(),

            highlight: true,
            theme: None,
            left_prompt: None,
            right_prompt: None,

            serve_addr: None,
            user_agent: None,
            save_shell_history: true,
            sync_models_url: None,

            clients: vec![],

            macro_flag: false,
            info_flag: false,
            agent_variables: None,

            model: Default::default(),
            functions: Default::default(),
            working_mode: WorkingMode::Cmd,
            last_message: None,

            role: None,
            session: None,
            rag: None,
            agent: None,
        }
    }
}

pub type GlobalConfig = Arc<RwLock<Config>>;

impl Config {
    pub async fn init(working_mode: WorkingMode, info_flag: bool) -> Result<Self> {
        let config_path = Self::config_file();
        let mut config = if !config_path.exists() {
            match env::var(get_env_name("provider"))
                .ok()
                .or_else(|| env::var(get_env_name("platform")).ok())
            {
                Some(v) => Self::load_dynamic(&v)?,
                None => {
                    if *IS_STDOUT_TERMINAL {
                        create_config_file(&config_path).await?;
                    }
                    Self::load_from_file(&config_path)?
                }
            }
        } else {
            Self::load_from_file(&config_path)?
        };

        config.working_mode = working_mode;
        config.info_flag = info_flag;

        let setup = |config: &mut Self| -> Result<()> {
            config.load_envs();

            if let Some(wrap) = config.wrap.clone() {
                config.set_wrap(&wrap)?;
            }

            config.load_functions()?;

            config.setup_model()?;
            config.setup_document_loaders();
            config.setup_user_agent();
            Ok(())
        };
        let ret = setup(&mut config);
        if !info_flag {
            ret?;
        }
        Ok(config)
    }

    pub fn config_dir() -> PathBuf {
        if let Ok(v) = env::var(get_env_name("config_dir")) {
            PathBuf::from(v)
        } else if let Ok(v) = env::var("XDG_CONFIG_HOME") {
            PathBuf::from(v).join(env!("CARGO_CRATE_NAME"))
        } else {
            let dir = dirs::config_dir().expect("No user's config directory");
            dir.join(env!("CARGO_CRATE_NAME"))
        }
    }

    pub fn local_path(name: &str) -> PathBuf {
        Self::config_dir().join(name)
    }

    pub fn config_file() -> PathBuf {
        match env::var(get_env_name("config_file")) {
            Ok(value) => PathBuf::from(value),
            Err(_) => Self::local_path(CONFIG_FILE_NAME),
        }
    }

    pub fn roles_dir() -> PathBuf {
        match env::var(get_env_name("roles_dir")) {
            Ok(value) => PathBuf::from(value),
            Err(_) => Self::local_path(ROLES_DIR_NAME),
        }
    }

    pub fn role_file(name: &str) -> PathBuf {
        Self::roles_dir().join(format!("{name}.md"))
    }

    pub fn macros_dir() -> PathBuf {
        match env::var(get_env_name("macros_dir")) {
            Ok(value) => PathBuf::from(value),
            Err(_) => Self::local_path(MACROS_DIR_NAME),
        }
    }

    pub fn macro_file(name: &str) -> PathBuf {
        Self::macros_dir().join(format!("{name}.yaml"))
    }

    pub fn env_file() -> PathBuf {
        match env::var(get_env_name("env_file")) {
            Ok(value) => PathBuf::from(value),
            Err(_) => Self::local_path(ENV_FILE_NAME),
        }
    }

    pub fn messages_file(&self) -> PathBuf {
        match &self.agent {
            None => match env::var(get_env_name("messages_file")) {
                Ok(value) => PathBuf::from(value),
                Err(_) => Self::local_path(MESSAGES_FILE_NAME),
            },
            Some(agent) => Self::agent_data_dir(agent.name()).join(MESSAGES_FILE_NAME),
        }
    }

    pub fn sessions_dir(&self) -> PathBuf {
        match &self.agent {
            None => match env::var(get_env_name("sessions_dir")) {
                Ok(value) => PathBuf::from(value),
                Err(_) => Self::local_path(SESSIONS_DIR_NAME),
            },
            Some(agent) => Self::agent_data_dir(agent.name()).join(SESSIONS_DIR_NAME),
        }
    }

    pub fn rags_dir() -> PathBuf {
        match env::var(get_env_name("rags_dir")) {
            Ok(value) => PathBuf::from(value),
            Err(_) => Self::local_path(RAGS_DIR_NAME),
        }
    }

    pub fn functions_dir() -> PathBuf {
        match env::var(get_env_name("functions_dir")) {
            Ok(value) => PathBuf::from(value),
            Err(_) => Self::local_path(FUNCTIONS_DIR_NAME),
        }
    }

    pub fn functions_file() -> PathBuf {
        Self::functions_dir().join(FUNCTIONS_FILE_NAME)
    }

    pub fn functions_bin_dir() -> PathBuf {
        Self::functions_dir().join(FUNCTIONS_BIN_DIR_NAME)
    }

    pub fn session_file(&self, name: &str) -> PathBuf {
        match name.split_once("/") {
            Some((dir, name)) => self.sessions_dir().join(dir).join(format!("{name}.yaml")),
            None => self.sessions_dir().join(format!("{name}.yaml")),
        }
    }

    pub fn rag_file(&self, name: &str) -> PathBuf {
        match &self.agent {
            Some(agent) => Self::agent_rag_file(agent.name(), name),
            None => Self::rags_dir().join(format!("{name}.yaml")),
        }
    }

    pub fn agents_data_dir() -> PathBuf {
        Self::local_path(AGENTS_DIR_NAME)
    }

    pub fn agent_data_dir(name: &str) -> PathBuf {
        match env::var(format!("{}_DATA_DIR", normalize_env_name(name))) {
            Ok(value) => PathBuf::from(value),
            Err(_) => Self::agents_data_dir().join(name),
        }
    }

    pub fn agent_config_file(name: &str) -> PathBuf {
        match env::var(format!("{}_CONFIG_FILE", normalize_env_name(name))) {
            Ok(value) => PathBuf::from(value),
            Err(_) => Self::agent_data_dir(name).join(CONFIG_FILE_NAME),
        }
    }

    pub fn agent_rag_file(agent_name: &str, rag_name: &str) -> PathBuf {
        Self::agent_data_dir(agent_name).join(format!("{rag_name}.yaml"))
    }

    pub fn agents_functions_dir() -> PathBuf {
        Self::functions_dir().join(AGENTS_DIR_NAME)
    }

    pub fn agent_functions_dir(name: &str) -> PathBuf {
        match env::var(format!("{}_FUNCTIONS_DIR", normalize_env_name(name))) {
            Ok(value) => PathBuf::from(value),
            Err(_) => Self::agents_functions_dir().join(name),
        }
    }

    pub fn models_override_file() -> PathBuf {
        Self::local_path("models-override.yaml")
    }

    pub fn state(&self) -> StateFlags {
        let mut flags = StateFlags::empty();
        if let Some(session) = &self.session {
            if session.is_empty() {
                flags |= StateFlags::SESSION_EMPTY;
            } else {
                flags |= StateFlags::SESSION;
            }
            if session.role_name().is_some() {
                flags |= StateFlags::ROLE;
            }
        } else if self.role.is_some() {
            flags |= StateFlags::ROLE;
        }
        if self.agent.is_some() {
            flags |= StateFlags::AGENT;
        }
        if self.rag.is_some() {
            flags |= StateFlags::RAG;
        }
        flags
    }

    pub fn serve_addr(&self) -> String {
        self.serve_addr.clone().unwrap_or_else(|| SERVE_ADDR.into())
    }

    pub fn log_config(is_serve: bool) -> Result<(LevelFilter, Option<PathBuf>)> {
        let log_level = env::var(get_env_name("log_level"))
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(match cfg!(debug_assertions) {
                true => LevelFilter::Debug,
                false => {
                    if is_serve {
                        LevelFilter::Info
                    } else {
                        LevelFilter::Off
                    }
                }
            });
        if log_level == LevelFilter::Off {
            return Ok((log_level, None));
        }
        let log_path = match env::var(get_env_name("log_path")) {
            Ok(v) => Some(PathBuf::from(v)),
            Err(_) => match is_serve {
                true => None,
                false => Some(Config::local_path(&format!(
                    "{}.log",
                    env!("CARGO_CRATE_NAME")
                ))),
            },
        };
        Ok((log_level, log_path))
    }

    pub fn edit_config(&self) -> Result<()> {
        let config_path = Self::config_file();
        let editor = self.editor()?;
        edit_file(&editor, &config_path)?;
        println!(
            "NOTE: Remember to restart {} if there are changes made to '{}",
            env!("CARGO_CRATE_NAME"),
            config_path.display(),
        );
        Ok(())
    }

    pub fn current_model(&self) -> &Model {
        if let Some(session) = self.session.as_ref() {
            session.model()
        } else if let Some(agent) = self.agent.as_ref() {
            agent.model()
        } else if let Some(role) = self.role.as_ref() {
            role.model()
        } else {
            &self.model
        }
    }

    pub fn role_like_mut(&mut self) -> Option<&mut dyn RoleLike> {
        if let Some(session) = self.session.as_mut() {
            Some(session)
        } else if let Some(agent) = self.agent.as_mut() {
            Some(agent)
        } else if let Some(role) = self.role.as_mut() {
            Some(role)
        } else {
            None
        }
    }

    pub fn extract_role(&self) -> Role {
        if let Some(session) = self.session.as_ref() {
            session.to_role()
        } else if let Some(agent) = self.agent.as_ref() {
            agent.to_role()
        } else if let Some(role) = self.role.as_ref() {
            role.clone()
        } else {
            let mut role = Role::default();
            role.batch_set(
                &self.model,
                self.temperature,
                self.top_p,
                self.use_tools.clone(),
            );
            role
        }
    }

    pub fn info(&self) -> Result<String> {
        if let Some(agent) = &self.agent {
            let output = agent.export()?;
            if let Some(session) = &self.session {
                let session = session
                    .export()?
                    .split('\n')
                    .map(|v| format!("  {v}"))
                    .collect::<Vec<_>>()
                    .join("\n");
                Ok(format!("{output}session:\n{session}"))
            } else {
                Ok(output)
            }
        } else if let Some(session) = &self.session {
            session.export()
        } else if let Some(role) = &self.role {
            Ok(role.export())
        } else if let Some(rag) = &self.rag {
            rag.export()
        } else {
            self.sysinfo()
        }
    }

    pub fn sysinfo(&self) -> Result<String> {
        let display_path = |path: &Path| path.display().to_string();
        let wrap = self
            .wrap
            .clone()
            .map_or_else(|| String::from("no"), |v| v.to_string());
        let (rag_reranker_model, rag_top_k) = match &self.rag {
            Some(rag) => rag.get_config(),
            None => (self.rag_reranker_model.clone(), self.rag_top_k),
        };
        let role = self.extract_role();
        let mut items = vec![
            ("model", role.model().id()),
            ("temperature", format_option_value(&role.temperature())),
            ("top_p", format_option_value(&role.top_p())),
            ("use_tools", format_option_value(&role.use_tools())),
            (
                "max_output_tokens",
                role.model()
                    .max_tokens_param()
                    .map(|v| format!("{v} (current model)"))
                    .unwrap_or_else(|| "null".into()),
            ),
            ("save_session", format_option_value(&self.save_session)),
            ("compress_threshold", self.compress_threshold.to_string()),
            (
                "rag_reranker_model",
                format_option_value(&rag_reranker_model),
            ),
            ("rag_top_k", rag_top_k.to_string()),
            ("dry_run", self.dry_run.to_string()),
            ("function_calling", self.function_calling.to_string()),
            ("stream", self.stream.to_string()),
            ("save", self.save.to_string()),
            ("keybindings", self.keybindings.clone()),
            ("wrap", wrap),
            ("wrap_code", self.wrap_code.to_string()),
            ("highlight", self.highlight.to_string()),
            ("theme", format_option_value(&self.theme)),
            ("config_file", display_path(&Self::config_file())),
            ("env_file", display_path(&Self::env_file())),
            ("roles_dir", display_path(&Self::roles_dir())),
            ("sessions_dir", display_path(&self.sessions_dir())),
            ("rags_dir", display_path(&Self::rags_dir())),
            ("macros_dir", display_path(&Self::macros_dir())),
            ("functions_dir", display_path(&Self::functions_dir())),
            ("messages_file", display_path(&self.messages_file())),
        ];
        if let Ok((_, Some(log_path))) = Self::log_config(self.working_mode.is_serve()) {
            items.push(("log_path", display_path(&log_path)));
        }
        let output = items
            .iter()
            .map(|(name, value)| format!("{name:<24}{value}\n"))
            .collect::<Vec<String>>()
            .join("");
        Ok(output)
    }

    pub fn update(config: &GlobalConfig, data: &str) -> Result<()> {
        let parts: Vec<&str> = data.split_whitespace().collect();
        if parts.len() != 2 {
            bail!("Usage: .set <key> <value>. If value is null, unset key.");
        }
        let key = parts[0];
        let value = parts[1];
        match key {
            "temperature" => {
                let value = parse_value(value)?;
                config.write().set_temperature(value);
            }
            "top_p" => {
                let value = parse_value(value)?;
                config.write().set_top_p(value);
            }
            "use_tools" => {
                let value = parse_value(value)?;
                config.write().set_use_tools(value);
            }
            "max_output_tokens" => {
                let value = parse_value(value)?;
                config.write().set_max_output_tokens(value);
            }
            "save_session" => {
                let value = parse_value(value)?;
                config.write().set_save_session(value);
            }
            "compress_threshold" => {
                let value = parse_value(value)?;
                config.write().set_compress_threshold(value);
            }
            "rag_reranker_model" => {
                let value = parse_value(value)?;
                Self::set_rag_reranker_model(config, value)?;
            }
            "rag_top_k" => {
                let value = value.parse().with_context(|| "Invalid value")?;
                Self::set_rag_top_k(config, value)?;
            }
            "dry_run" => {
                let value = value.parse().with_context(|| "Invalid value")?;
                config.write().dry_run = value;
            }
            "function_calling" => {
                let value = value.parse().with_context(|| "Invalid value")?;
                if value && config.write().functions.is_empty() {
                    bail!("Function calling cannot be enabled because no functions are installed.")
                }
                config.write().function_calling = value;
            }
            "stream" => {
                let value = value.parse().with_context(|| "Invalid value")?;
                config.write().stream = value;
            }
            "save" => {
                let value = value.parse().with_context(|| "Invalid value")?;
                config.write().save = value;
            }
            "highlight" => {
                let value = value.parse().with_context(|| "Invalid value")?;
                config.write().highlight = value;
            }
            _ => bail!("Unknown key '{key}'"),
        }
        Ok(())
    }

    pub fn delete(config: &GlobalConfig, kind: &str) -> Result<()> {
        let (dir, file_ext) = match kind {
            "role" => (Self::roles_dir(), Some(".md")),
            "session" => (config.read().sessions_dir(), Some(".yaml")),
            "rag" => (Self::rags_dir(), Some(".yaml")),
            "macro" => (Self::macros_dir(), Some(".yaml")),
            "agent-data" => (Self::agents_data_dir(), None),
            _ => bail!("Unknown kind '{kind}'"),
        };
        let names = match read_dir(&dir) {
            Ok(rd) => {
                let mut names = vec![];
                for entry in rd.flatten() {
                    let name = entry.file_name();
                    match file_ext {
                        Some(file_ext) => {
                            if let Some(name) = name.to_string_lossy().strip_suffix(file_ext) {
                                names.push(name.to_string());
                            }
                        }
                        None => {
                            if entry.path().is_dir() {
                                names.push(name.to_string_lossy().to_string());
                            }
                        }
                    }
                }
                names.sort_unstable();
                names
            }
            Err(_) => vec![],
        };

        if names.is_empty() {
            bail!("No {kind} to delete")
        }

        let select_names = MultiSelect::new(&format!("Select {kind} to delete:"), names)
            .with_validator(|list: &[ListOption<&String>]| {
                if list.is_empty() {
                    Ok(Validation::Invalid(
                        "At least one item must be selected".into(),
                    ))
                } else {
                    Ok(Validation::Valid)
                }
            })
            .prompt()?;

        for name in select_names {
            match file_ext {
                Some(ext) => {
                    let path = dir.join(format!("{name}{ext}"));
                    remove_file(&path).with_context(|| {
                        format!("Failed to delete {kind} at '{}'", path.display())
                    })?;
                }
                None => {
                    let path = dir.join(name);
                    remove_dir_all(&path).with_context(|| {
                        format!("Failed to delete {kind} at '{}'", path.display())
                    })?;
                }
            }
        }
        println!("✓ Successfully deleted {kind}.");
        Ok(())
    }

    pub fn set_temperature(&mut self, value: Option<f64>) {
        match self.role_like_mut() {
            Some(role_like) => role_like.set_temperature(value),
            None => self.temperature = value,
        }
    }

    pub fn set_top_p(&mut self, value: Option<f64>) {
        match self.role_like_mut() {
            Some(role_like) => role_like.set_top_p(value),
            None => self.top_p = value,
        }
    }

    pub fn set_use_tools(&mut self, value: Option<String>) {
        match self.role_like_mut() {
            Some(role_like) => role_like.set_use_tools(value),
            None => self.use_tools = value,
        }
    }

    pub fn set_save_session(&mut self, value: Option<bool>) {
        if let Some(session) = self.session.as_mut() {
            session.set_save_session(value);
        } else {
            self.save_session = value;
        }
    }

    pub fn set_compress_threshold(&mut self, value: Option<usize>) {
        if let Some(session) = self.session.as_mut() {
            session.set_compress_threshold(value);
        } else {
            self.compress_threshold = value.unwrap_or_default();
        }
    }

    pub fn set_rag_reranker_model(config: &GlobalConfig, value: Option<String>) -> Result<()> {
        if let Some(id) = &value {
            Model::retrieve_model(&config.read(), id, ModelType::Reranker)?;
        }
        let has_rag = config.read().rag.is_some();
        match has_rag {
            true => update_rag(config, |rag| {
                rag.set_reranker_model(value)?;
                Ok(())
            })?,
            false => config.write().rag_reranker_model = value,
        }
        Ok(())
    }

    pub fn set_rag_top_k(config: &GlobalConfig, value: usize) -> Result<()> {
        let has_rag = config.read().rag.is_some();
        match has_rag {
            true => update_rag(config, |rag| {
                rag.set_top_k(value)?;
                Ok(())
            })?,
            false => config.write().rag_top_k = value,
        }
        Ok(())
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

    pub fn set_max_output_tokens(&mut self, value: Option<isize>) {
        match self.role_like_mut() {
            Some(role_like) => {
                let mut model = role_like.model().clone();
                model.set_max_tokens(value, true);
                role_like.set_model(model);
            }
            None => {
                self.model.set_max_tokens(value, true);
            }
        };
    }

    pub fn set_model(&mut self, model_id: &str) -> Result<()> {
        let model = Model::retrieve_model(self, model_id, ModelType::Chat)?;
        match self.role_like_mut() {
            Some(role_like) => role_like.set_model(model),
            None => {
                self.model = model;
            }
        }
        Ok(())
    }

    pub fn use_prompt(&mut self, prompt: &str) -> Result<()> {
        let mut role = Role::new(TEMP_ROLE_NAME, prompt);
        role.set_model(self.current_model().clone());
        self.use_role_obj(role)
    }

    pub fn use_role(&mut self, name: &str) -> Result<()> {
        let role = self.retrieve_role(name)?;
        self.use_role_obj(role)
    }

    pub fn use_role_obj(&mut self, role: Role) -> Result<()> {
        if self.agent.is_some() {
            bail!("Cannot perform this operation because you are using a agent")
        }
        if let Some(session) = self.session.as_mut() {
            session.guard_empty()?;
            session.set_role(role);
        } else {
            self.role = Some(role);
        }
        Ok(())
    }

    pub fn role_info(&self) -> Result<String> {
        if let Some(session) = &self.session {
            if session.role_name().is_some() {
                let role = session.to_role();
                Ok(role.export())
            } else {
                bail!("No session role")
            }
        } else if let Some(role) = &self.role {
            Ok(role.export())
        } else {
            bail!("No role")
        }
    }

    pub fn exit_role(&mut self) -> Result<()> {
        if let Some(session) = self.session.as_mut() {
            session.guard_empty()?;
            session.clear_role();
        } else if self.role.is_some() {
            self.role = None;
        }
        Ok(())
    }

    pub fn retrieve_role(&self, name: &str) -> Result<Role> {
        let names = Self::list_roles(false);
        let mut role = if names.contains(&name.to_string()) {
            let path = Self::role_file(name);
            let content = read_to_string(&path)?;
            Role::new(name, &content)
        } else {
            Role::builtin(name)?
        };
        let current_model = self.current_model().clone();
        match role.model_id() {
            Some(model_id) => {
                if current_model.id() != model_id {
                    let model = Model::retrieve_model(self, model_id, ModelType::Chat)?;
                    role.set_model(model);
                } else {
                    role.set_model(current_model);
                }
            }
            None => {
                role.set_model(current_model);
                if role.temperature().is_none() {
                    role.set_temperature(self.temperature);
                }
                if role.top_p().is_none() {
                    role.set_top_p(self.top_p);
                }
            }
        }
        Ok(role)
    }

    pub fn new_role(&mut self, name: &str) -> Result<()> {
        if self.macro_flag {
            bail!("No role");
        }
        let ans = Confirm::new("Create a new role?")
            .with_default(true)
            .prompt()?;
        if ans {
            self.upsert_role(name)?;
        } else {
            bail!("No role");
        }
        Ok(())
    }

    pub fn edit_role(&mut self) -> Result<()> {
        let role_name;
        if let Some(session) = self.session.as_ref() {
            if let Some(name) = session.role_name().map(|v| v.to_string()) {
                if session.is_empty() {
                    role_name = Some(name);
                } else {
                    bail!("Cannot perform this operation because you are in a non-empty session")
                }
            } else {
                bail!("No role")
            }
        } else {
            role_name = self.role.as_ref().map(|v| v.name().to_string());
        }
        let name = role_name.ok_or_else(|| anyhow!("No role"))?;
        self.upsert_role(&name)?;
        self.use_role(&name)
    }

    pub fn upsert_role(&mut self, name: &str) -> Result<()> {
        let role_path = Self::role_file(name);
        ensure_parent_exists(&role_path)?;
        let editor = self.editor()?;
        edit_file(&editor, &role_path)?;
        if self.working_mode.is_repl() {
            println!("✓ Saved the role to '{}'.", role_path.display());
        }
        Ok(())
    }

    pub fn save_role(&mut self, name: Option<&str>) -> Result<()> {
        let mut role_name = match &self.role {
            Some(role) => {
                if role.has_args() {
                    bail!("Unable to save the role with arguments (whose name contains '#')")
                }
                match name {
                    Some(v) => v.to_string(),
                    None => role.name().to_string(),
                }
            }
            None => bail!("No role"),
        };
        if role_name == TEMP_ROLE_NAME {
            role_name = Text::new("Role name:")
                .with_validator(|input: &str| {
                    let input = input.trim();
                    if input.is_empty() {
                        Ok(Validation::Invalid("This name is required".into()))
                    } else if input == TEMP_ROLE_NAME {
                        Ok(Validation::Invalid("This name is reserved".into()))
                    } else {
                        Ok(Validation::Valid)
                    }
                })
                .prompt()?;
        }
        let role_path = Self::role_file(&role_name);
        if let Some(role) = self.role.as_mut() {
            role.save(&role_name, &role_path, self.working_mode.is_repl())?;
        }

        Ok(())
    }

    pub fn all_roles() -> Vec<Role> {
        let mut roles: HashMap<String, Role> = Role::list_builtin_roles()
            .iter()
            .map(|v| (v.name().to_string(), v.clone()))
            .collect();
        let names = Self::list_roles(false);
        for name in names {
            if let Ok(content) = read_to_string(Self::role_file(&name)) {
                let role = Role::new(&name, &content);
                roles.insert(name, role);
            }
        }
        let mut roles: Vec<_> = roles.into_values().collect();
        roles.sort_unstable_by(|a, b| a.name().cmp(b.name()));
        roles
    }

    pub fn list_roles(with_builtin: bool) -> Vec<String> {
        let mut names = HashSet::new();
        if let Ok(rd) = read_dir(Self::roles_dir()) {
            for entry in rd.flatten() {
                if let Some(name) = entry
                    .file_name()
                    .to_str()
                    .and_then(|v| v.strip_suffix(".md"))
                {
                    names.insert(name.to_string());
                }
            }
        }
        if with_builtin {
            names.extend(Role::list_builtin_role_names());
        }
        let mut names: Vec<_> = names.into_iter().collect();
        names.sort_unstable();
        names
    }

    pub fn has_role(name: &str) -> bool {
        let names = Self::list_roles(true);
        names.contains(&name.to_string())
    }

    pub fn use_session(&mut self, session_name: Option<&str>) -> Result<()> {
        if self.session.is_some() {
            bail!(
                "Already in a session, please run '.exit session' first to exit the current session."
            );
        }
        let mut session;
        match session_name {
            None | Some(TEMP_SESSION_NAME) => {
                let session_file = self.session_file(TEMP_SESSION_NAME);
                if session_file.exists() {
                    remove_file(session_file).with_context(|| {
                        format!("Failed to cleanup previous '{TEMP_SESSION_NAME}' session")
                    })?;
                }
                session = Some(Session::new(self, TEMP_SESSION_NAME));
            }
            Some(name) => {
                let session_path = self.session_file(name);
                if !session_path.exists() {
                    session = Some(Session::new(self, name));
                } else {
                    session = Some(Session::load(self, name, &session_path)?);
                }
            }
        }
        let mut new_session = false;
        if let Some(session) = session.as_mut() {
            if session.is_empty() {
                new_session = true;
                if let Some(LastMessage {
                    input,
                    output,
                    continuous,
                }) = &self.last_message
                {
                    if (*continuous && !output.is_empty())
                        && self.agent.is_some() == input.with_agent()
                    {
                        let ans = Confirm::new(
                            "Start a session that incorporates the last question and answer?",
                        )
                        .with_default(false)
                        .prompt()?;
                        if ans {
                            session.add_message(input, output)?;
                        }
                    }
                }
            }
        }
        self.session = session;
        self.init_agent_session_variables(new_session)?;
        Ok(())
    }

    pub fn session_info(&self) -> Result<String> {
        if let Some(session) = &self.session {
            let render_options = self.render_options()?;
            let mut markdown_render = MarkdownRender::init(render_options)?;
            let agent_info: Option<(String, Vec<String>)> = self.agent.as_ref().map(|agent| {
                let functions = agent
                    .functions()
                    .declarations()
                    .iter()
                    .filter_map(|v| if v.agent { Some(v.name.clone()) } else { None })
                    .collect();
                (agent.name().to_string(), functions)
            });
            session.render(&mut markdown_render, &agent_info)
        } else {
            bail!("No session")
        }
    }

    pub fn exit_session(&mut self) -> Result<()> {
        if let Some(mut session) = self.session.take() {
            let sessions_dir = self.sessions_dir();
            session.exit(&sessions_dir, self.working_mode.is_repl())?;
            self.discontinuous_last_message();
        }
        Ok(())
    }

    pub fn save_session(&mut self, name: Option<&str>) -> Result<()> {
        let session_name = match &self.session {
            Some(session) => match name {
                Some(v) => v.to_string(),
                None => session
                    .autoname()
                    .unwrap_or_else(|| session.name())
                    .to_string(),
            },
            None => bail!("No session"),
        };
        let session_path = self.session_file(&session_name);
        if let Some(session) = self.session.as_mut() {
            session.save(&session_name, &session_path, self.working_mode.is_repl())?;
        }
        Ok(())
    }

    pub fn edit_session(&mut self) -> Result<()> {
        let name = match &self.session {
            Some(session) => session.name().to_string(),
            None => bail!("No session"),
        };
        let session_path = self.session_file(&name);
        self.save_session(Some(&name))?;
        let editor = self.editor()?;
        edit_file(&editor, &session_path).with_context(|| {
            format!(
                "Failed to edit '{}' with '{editor}'",
                session_path.display()
            )
        })?;
        self.session = Some(Session::load(self, &name, &session_path)?);
        self.discontinuous_last_message();
        Ok(())
    }

    pub fn empty_session(&mut self) -> Result<()> {
        if let Some(session) = self.session.as_mut() {
            if let Some(agent) = self.agent.as_ref() {
                session.sync_agent(agent);
            }
            session.clear_messages();
        } else {
            bail!("No session")
        }
        self.discontinuous_last_message();
        Ok(())
    }

    pub fn set_save_session_this_time(&mut self) -> Result<()> {
        if let Some(session) = self.session.as_mut() {
            session.set_save_session_this_time();
        } else {
            bail!("No session")
        }
        Ok(())
    }

    pub fn list_sessions(&self) -> Vec<String> {
        list_file_names(self.sessions_dir(), ".yaml")
    }

    pub fn list_autoname_sessions(&self) -> Vec<String> {
        list_file_names(self.sessions_dir().join("_"), ".yaml")
    }

    pub fn maybe_compress_session(config: GlobalConfig) {
        let mut need_compress = false;
        {
            let mut config = config.write();
            let compress_threshold = config.compress_threshold;
            if let Some(session) = config.session.as_mut() {
                if session.need_compress(compress_threshold) {
                    session.set_compressing(true);
                    need_compress = true;
                }
            }
        };
        if !need_compress {
            return;
        }
        let color = if config.read().light_theme() {
            nu_ansi_term::Color::LightGray
        } else {
            nu_ansi_term::Color::DarkGray
        };
        print!(
            "\n📢 {}\n",
            color.italic().paint("Compressing the session."),
        );
        tokio::spawn(async move {
            if let Err(err) = Config::compress_session(&config).await {
                warn!("Failed to compress the session: {err}");
            }
            if let Some(session) = config.write().session.as_mut() {
                session.set_compressing(false);
            }
        });
    }

    pub async fn compress_session(config: &GlobalConfig) -> Result<()> {
        match config.read().session.as_ref() {
            Some(session) => {
                if !session.has_user_messages() {
                    bail!("No need to compress since there are no messages in the session")
                }
            }
            None => bail!("No session"),
        }

        let prompt = config
            .read()
            .summarize_prompt
            .clone()
            .unwrap_or_else(|| SUMMARIZE_PROMPT.into());
        let input = Input::from_str(config, &prompt, None);
        let summary = input.fetch_chat_text().await?;
        let summary_prompt = config
            .read()
            .summary_prompt
            .clone()
            .unwrap_or_else(|| SUMMARY_PROMPT.into());
        if let Some(session) = config.write().session.as_mut() {
            session.compress(format!("{summary_prompt}{summary}"));
        }
        config.write().discontinuous_last_message();
        Ok(())
    }

    pub fn is_compressing_session(&self) -> bool {
        self.session
            .as_ref()
            .map(|v| v.compressing())
            .unwrap_or_default()
    }

    pub fn maybe_autoname_session(config: GlobalConfig) {
        let mut need_autoname = false;
        if let Some(session) = config.write().session.as_mut() {
            if session.need_autoname() {
                session.set_autonaming(true);
                need_autoname = true;
            }
        }
        if !need_autoname {
            return;
        }
        let color = if config.read().light_theme() {
            nu_ansi_term::Color::LightGray
        } else {
            nu_ansi_term::Color::DarkGray
        };
        print!("\n📢 {}\n", color.italic().paint("Autonaming the session."),);
        tokio::spawn(async move {
            if let Err(err) = Config::autoname_session(&config).await {
                warn!("Failed to autonaming the session: {err}");
            }
            if let Some(session) = config.write().session.as_mut() {
                session.set_autonaming(false);
            }
        });
    }

    pub async fn autoname_session(config: &GlobalConfig) -> Result<()> {
        let text = match config
            .read()
            .session
            .as_ref()
            .and_then(|v| v.chat_history_for_autonaming())
        {
            Some(v) => v,
            None => bail!("No chat history"),
        };
        let role = config.read().retrieve_role(CREATE_TITLE_ROLE)?;
        let input = Input::from_str(config, &text, Some(role));
        let text = input.fetch_chat_text().await?;
        if let Some(session) = config.write().session.as_mut() {
            session.set_autoname(&text);
        }
        Ok(())
    }

    pub async fn use_rag(
        config: &GlobalConfig,
        rag: Option<&str>,
        abort_signal: AbortSignal,
    ) -> Result<()> {
        if config.read().agent.is_some() {
            bail!("Cannot perform this operation because you are using a agent")
        }
        let rag = match rag {
            None => {
                let rag_path = config.read().rag_file(TEMP_RAG_NAME);
                if rag_path.exists() {
                    remove_file(&rag_path).with_context(|| {
                        format!("Failed to cleanup previous '{TEMP_RAG_NAME}' rag")
                    })?;
                }
                Rag::init(config, TEMP_RAG_NAME, &rag_path, &[], abort_signal).await?
            }
            Some(name) => {
                let rag_path = config.read().rag_file(name);
                if !rag_path.exists() {
                    if config.read().working_mode.is_cmd() {
                        bail!("Unknown RAG '{name}'")
                    }
                    Rag::init(config, name, &rag_path, &[], abort_signal).await?
                } else {
                    Rag::load(config, name, &rag_path)?
                }
            }
        };
        config.write().rag = Some(Arc::new(rag));
        Ok(())
    }

    pub async fn edit_rag_docs(config: &GlobalConfig, abort_signal: AbortSignal) -> Result<()> {
        let mut rag = match config.read().rag.clone() {
            Some(v) => v.as_ref().clone(),
            None => bail!("No RAG"),
        };

        let document_paths = rag.document_paths();
        let temp_file = temp_file(&format!("-rag-{}", rag.name()), ".txt");
        tokio::fs::write(&temp_file, &document_paths.join("\n"))
            .await
            .with_context(|| format!("Failed to write to '{}'", temp_file.display()))?;
        let editor = config.read().editor()?;
        edit_file(&editor, &temp_file)?;
        let new_document_paths = tokio::fs::read_to_string(&temp_file)
            .await
            .with_context(|| format!("Failed to read '{}'", temp_file.display()))?;
        let new_document_paths = new_document_paths
            .split('\n')
            .filter_map(|v| {
                let v = v.trim();
                if v.is_empty() {
                    None
                } else {
                    Some(v.to_string())
                }
            })
            .collect::<Vec<_>>();
        if new_document_paths.is_empty() || new_document_paths == document_paths {
            bail!("No changes")
        }
        rag.refresh_document_paths(&new_document_paths, false, config, abort_signal)
            .await?;
        config.write().rag = Some(Arc::new(rag));
        Ok(())
    }

    pub async fn rebuild_rag(config: &GlobalConfig, abort_signal: AbortSignal) -> Result<()> {
        let mut rag = match config.read().rag.clone() {
            Some(v) => v.as_ref().clone(),
            None => bail!("No RAG"),
        };
        let document_paths = rag.document_paths().to_vec();
        rag.refresh_document_paths(&document_paths, true, config, abort_signal)
            .await?;
        config.write().rag = Some(Arc::new(rag));
        Ok(())
    }

    pub fn rag_sources(config: &GlobalConfig) -> Result<String> {
        match config.read().rag.as_ref() {
            Some(rag) => match rag.get_last_sources() {
                Some(v) => Ok(v),
                None => bail!("No sources"),
            },
            None => bail!("No RAG"),
        }
    }

    pub fn rag_info(&self) -> Result<String> {
        if let Some(rag) = &self.rag {
            rag.export()
        } else {
            bail!("No RAG")
        }
    }

    pub fn exit_rag(&mut self) -> Result<()> {
        self.rag.take();
        Ok(())
    }

    pub async fn search_rag(
        config: &GlobalConfig,
        rag: &Rag,
        text: &str,
        abort_signal: AbortSignal,
    ) -> Result<String> {
        let (reranker_model, top_k) = rag.get_config();
        let (embeddings, ids) = rag
            .search(text, top_k, reranker_model.as_deref(), abort_signal)
            .await?;
        let text = config.read().rag_template(&embeddings, text);
        rag.set_last_sources(&ids);
        Ok(text)
    }

    pub fn list_rags() -> Vec<String> {
        match read_dir(Self::rags_dir()) {
            Ok(rd) => {
                let mut names = vec![];
                for entry in rd.flatten() {
                    let name = entry.file_name();
                    if let Some(name) = name.to_string_lossy().strip_suffix(".yaml") {
                        names.push(name.to_string());
                    }
                }
                names.sort_unstable();
                names
            }
            Err(_) => vec![],
        }
    }

    pub fn rag_template(&self, embeddings: &str, text: &str) -> String {
        if embeddings.is_empty() {
            return text.to_string();
        }
        self.rag_template
            .as_deref()
            .unwrap_or(RAG_TEMPLATE)
            .replace("__CONTEXT__", embeddings)
            .replace("__INPUT__", text)
    }

    pub async fn use_agent(
        config: &GlobalConfig,
        agent_name: &str,
        session_name: Option<&str>,
        abort_signal: AbortSignal,
    ) -> Result<()> {
        if !config.read().function_calling {
            bail!("Please enable function calling before using the agent.");
        }
        if config.read().agent.is_some() {
            bail!("Already in a agent, please run '.exit agent' first to exit the current agent.");
        }
        let agent = Agent::init(config, agent_name, abort_signal).await?;
        let session = session_name.map(|v| v.to_string()).or_else(|| {
            if config.read().macro_flag {
                None
            } else {
                agent.agent_prelude().map(|v| v.to_string())
            }
        });
        config.write().rag = agent.rag();
        config.write().agent = Some(agent);
        if let Some(session) = session {
            config.write().use_session(Some(&session))?;
        } else {
            config.write().init_agent_shared_variables()?;
        }
        Ok(())
    }

    pub fn agent_info(&self) -> Result<String> {
        if let Some(agent) = &self.agent {
            agent.export()
        } else {
            bail!("No agent")
        }
    }

    pub fn agent_banner(&self) -> Result<String> {
        if let Some(agent) = &self.agent {
            Ok(agent.banner())
        } else {
            bail!("No agent")
        }
    }

    pub fn edit_agent_config(&self) -> Result<()> {
        let agent_name = match &self.agent {
            Some(agent) => agent.name(),
            None => bail!("No agent"),
        };
        let agent_config_path = Config::agent_config_file(agent_name);
        ensure_parent_exists(&agent_config_path)?;
        if !agent_config_path.exists() {
            std::fs::write(
                &agent_config_path,
                "# see https://github.com/sigoden/aichat/blob/main/config.agent.example.yaml\n",
            )
            .with_context(|| format!("Failed to write to '{}'", agent_config_path.display()))?;
        }
        let editor = self.editor()?;
        edit_file(&editor, &agent_config_path)?;
        println!(
            "NOTE: Remember to reload the agent if there are changes made to '{}'",
            agent_config_path.display()
        );
        Ok(())
    }

    pub fn exit_agent(&mut self) -> Result<()> {
        self.exit_session()?;
        if self.agent.take().is_some() {
            self.rag.take();
            self.discontinuous_last_message();
        }
        Ok(())
    }

    pub fn exit_agent_session(&mut self) -> Result<()> {
        self.exit_session()?;
        if let Some(agent) = self.agent.as_mut() {
            agent.exit_session();
            if self.working_mode.is_repl() {
                self.init_agent_shared_variables()?;
            }
        }
        Ok(())
    }

    pub fn list_macros() -> Vec<String> {
        list_file_names(Self::macros_dir(), ".yaml")
    }

    pub fn load_macro(name: &str) -> Result<Macro> {
        let path = Self::macro_file(name);
        let err = || format!("Failed to load macro '{name}' at '{}'", path.display());
        let content = read_to_string(&path).with_context(err)?;
        let value: Macro = serde_yaml::from_str(&content).with_context(err)?;
        Ok(value)
    }

    pub fn has_macro(name: &str) -> bool {
        let names = Self::list_macros();
        names.contains(&name.to_string())
    }

    pub fn new_macro(&mut self, name: &str) -> Result<()> {
        if self.macro_flag {
            bail!("No macro");
        }
        let ans = Confirm::new("Create a new macro?")
            .with_default(true)
            .prompt()?;
        if ans {
            let macro_path = Self::macro_file(name);
            ensure_parent_exists(&macro_path)?;
            let editor = self.editor()?;
            edit_file(&editor, &macro_path)?;
        } else {
            bail!("No macro");
        }
        Ok(())
    }

    pub fn apply_prelude(&mut self) -> Result<()> {
        if self.macro_flag || !self.state().is_empty() {
            return Ok(());
        }
        let prelude = match self.working_mode {
            WorkingMode::Repl => self.repl_prelude.as_ref(),
            WorkingMode::Cmd => self.cmd_prelude.as_ref(),
            WorkingMode::Serve => return Ok(()),
        };
        let prelude = match prelude {
            Some(v) => {
                if v.is_empty() {
                    return Ok(());
                }
                v.to_string()
            }
            None => return Ok(()),
        };

        let err_msg = || format!("Invalid prelude '{prelude}");
        match prelude.split_once(':') {
            Some(("role", name)) => {
                self.use_role(name).with_context(err_msg)?;
            }
            Some(("session", name)) => {
                self.use_session(Some(name)).with_context(err_msg)?;
            }
            Some((session_name, role_name)) => {
                self.use_session(Some(session_name)).with_context(err_msg)?;
                if let Some(true) = self.session.as_ref().map(|v| v.is_empty()) {
                    self.use_role(role_name).with_context(err_msg)?;
                }
            }
            _ => {
                bail!("{}", err_msg())
            }
        }
        Ok(())
    }

    pub fn select_functions(&self, role: &Role) -> Option<Vec<FunctionDeclaration>> {
        let mut functions = vec![];
        if self.function_calling {
            let use_tools = role.use_tools().map(|v| v.to_string()).or_else(|| {
                if role.model().client_name() == "jules" {
                    Some("all".to_string())
                } else {
                    None
                }
            });
            if let Some(use_tools) = &use_tools {
                let mut tool_names: HashSet<String> = Default::default();
                let declaration_names: HashSet<String> = self
                    .functions
                    .declarations()
                    .iter()
                    .map(|v| v.name.to_string())
                    .collect();
                if use_tools == "all" {
                    tool_names.extend(declaration_names);
                } else {
                    for item in use_tools.split(',') {
                        let item = item.trim();
                        if let Some(values) = self.mapping_tools.get(item) {
                            tool_names.extend(
                                values
                                    .split(',')
                                    .map(|v| v.to_string())
                                    .filter(|v| declaration_names.contains(v)),
                            )
                        } else if declaration_names.contains(item) {
                            tool_names.insert(item.to_string());
                        }
                    }
                }
                functions = self
                    .functions
                    .declarations()
                    .iter()
                    .filter_map(|v| {
                        if tool_names.contains(&v.name) {
                            Some(v.clone())
                        } else {
                            None
                        }
                    })
                    .collect();
            }

            if let Some(agent) = &self.agent {
                let mut agent_functions = agent.functions().declarations().to_vec();
                let tool_names: HashSet<String> = agent_functions
                    .iter()
                    .filter_map(|v| {
                        if v.agent {
                            None
                        } else {
                            Some(v.name.to_string())
                        }
                    })
                    .collect();
                agent_functions.extend(
                    functions
                        .into_iter()
                        .filter(|v| !tool_names.contains(&v.name)),
                );
                functions = agent_functions;
            }
        };
        if functions.is_empty() {
            None
        } else {
            Some(functions)
        }
    }

    pub fn editor(&self) -> Result<String> {
        EDITOR.get_or_init(move || {
            let editor = self.editor.clone()
                .or_else(|| env::var("VISUAL").ok().or_else(|| env::var("EDITOR").ok()))
                .unwrap_or_else(|| {
                    if cfg!(windows) {
                        "notepad".to_string()
                    } else {
                        "nano".to_string()
                    }
                });
            which::which(&editor).ok().map(|_| editor)
        })
        .clone()
        .ok_or_else(|| anyhow!("Editor not found. Please add the `editor` configuration or set the $EDITOR or $VISUAL environment variable."))
    }

    pub fn repl_complete(
        &self,
        cmd: &str,
        args: &[&str],
        _line: &str,
    ) -> Vec<(String, Option<String>)> {
        let mut values: Vec<(String, Option<String>)> = vec![];
        let filter = args.last().unwrap_or(&"");
        if args.len() == 1 {
            values = match cmd {
                ".role" => map_completion_values(Self::list_roles(true)),
                ".model" => list_models(self, ModelType::Chat)
                    .into_iter()
                    .map(|v| (v.id(), Some(v.description())))
                    .collect(),
                ".session" => {
                    if args[0].starts_with("_/") {
                        map_completion_values(
                            self.list_autoname_sessions()
                                .iter()
                                .rev()
                                .map(|v| format!("_/{v}"))
                                .collect::<Vec<String>>(),
                        )
                    } else {
                        map_completion_values(self.list_sessions())
                    }
                }
                ".rag" => map_completion_values(Self::list_rags()),
                ".agent" => map_completion_values(list_agents()),
                ".macro" => map_completion_values(Self::list_macros()),
                ".starter" => match &self.agent {
                    Some(agent) => agent
                        .conversation_staters()
                        .iter()
                        .enumerate()
                        .map(|(i, v)| ((i + 1).to_string(), Some(v.to_string())))
                        .collect(),
                    None => vec![],
                },
                ".set" => {
                    let mut values = vec![
                        "temperature",
                        "top_p",
                        "use_tools",
                        "save_session",
                        "compress_threshold",
                        "rag_reranker_model",
                        "rag_top_k",
                        "max_output_tokens",
                        "dry_run",
                        "function_calling",
                        "stream",
                        "save",
                        "highlight",
                    ];
                    values.sort_unstable();
                    values
                        .into_iter()
                        .map(|v| (format!("{v} "), None))
                        .collect()
                }
                ".delete" => {
                    map_completion_values(vec!["role", "session", "rag", "macro", "agent-data"])
                }
                _ => vec![],
            };
        } else if cmd == ".set" && args.len() == 2 {
            let candidates = match args[0] {
                "max_output_tokens" => match self.current_model().max_output_tokens() {
                    Some(v) => vec![v.to_string()],
                    None => vec![],
                },
                "dry_run" => complete_bool(self.dry_run),
                "stream" => complete_bool(self.stream),
                "save" => complete_bool(self.save),
                "function_calling" => complete_bool(self.function_calling),
                "use_tools" => {
                    let mut prefix = String::new();
                    let mut ignores = HashSet::new();
                    if let Some((v, _)) = args[1].rsplit_once(',') {
                        ignores = v.split(',').collect();
                        prefix = format!("{v},");
                    }
                    let mut values = vec![];
                    if prefix.is_empty() {
                        values.push("all".to_string());
                    }
                    values.extend(self.functions.declarations().iter().map(|v| v.name.clone()));
                    values.extend(self.mapping_tools.keys().map(|v| v.to_string()));
                    values
                        .into_iter()
                        .filter(|v| !ignores.contains(v.as_str()))
                        .map(|v| format!("{prefix}{v}"))
                        .collect()
                }
                "save_session" => {
                    let save_session = if let Some(session) = &self.session {
                        session.save_session()
                    } else {
                        self.save_session
                    };
                    complete_option_bool(save_session)
                }
                "rag_reranker_model" => list_models(self, ModelType::Reranker)
                    .iter()
                    .map(|v| v.id())
                    .collect(),
                "highlight" => complete_bool(self.highlight),
                _ => vec![],
            };
            values = candidates.into_iter().map(|v| (v, None)).collect();
        } else if cmd == ".agent" {
            if args.len() == 2 {
                let dir = Self::agent_data_dir(args[0]).join(SESSIONS_DIR_NAME);
                values = list_file_names(dir, ".yaml")
                    .into_iter()
                    .map(|v| (v, None))
                    .collect();
            }
            values.extend(complete_agent_variables(args[0]));
        };
        fuzzy_filter(values, |v| v.0.as_str(), filter)
    }

    pub fn sync_models_url(&self) -> String {
        self.sync_models_url
            .clone()
            .unwrap_or_else(|| SYNC_MODELS_URL.into())
    }

    pub async fn sync_models(url: &str, abort_signal: AbortSignal) -> Result<()> {
        let content = abortable_run_with_spinner(fetch(url), "Fetching models.yaml", abort_signal)
            .await
            .with_context(|| format!("Failed to fetch '{url}'"))?;
        println!("✓ Fetched '{url}'");
        let list = serde_yaml::from_str::<Vec<ProviderModels>>(&content)
            .with_context(|| "Failed to parse models.yaml")?;
        let models_override = ModelsOverride {
            version: env!("CARGO_PKG_VERSION").to_string(),
            list,
        };
        let models_override_data =
            serde_yaml::to_string(&models_override).with_context(|| "Failed to serde {}")?;

        let model_override_path = Self::models_override_file();
        ensure_parent_exists(&model_override_path)?;
        std::fs::write(&model_override_path, models_override_data)
            .with_context(|| format!("Failed to write to '{}'", model_override_path.display()))?;
        println!("✓ Updated '{}'", model_override_path.display());
        Ok(())
    }

    pub fn loal_models_override() -> Result<Vec<ProviderModels>> {
        let model_override_path = Self::models_override_file();
        let err = || {
            format!(
                "Failed to load models at '{}'",
                model_override_path.display()
            )
        };
        let content = read_to_string(&model_override_path).with_context(err)?;
        let models_override: ModelsOverride = serde_yaml::from_str(&content).with_context(err)?;
        if models_override.version != env!("CARGO_PKG_VERSION") {
            bail!("Incompatible version")
        }
        Ok(models_override.list)
    }

    pub fn light_theme(&self) -> bool {
        matches!(self.theme.as_deref(), Some("light"))
    }

    pub fn render_options(&self) -> Result<RenderOptions> {
        let theme = if self.highlight {
            let theme_mode = if self.light_theme() { "light" } else { "dark" };
            let theme_filename = format!("{theme_mode}.tmTheme");
            let theme_path = Self::local_path(&theme_filename);
            if theme_path.exists() {
                let theme = ThemeSet::get_theme(&theme_path)
                    .with_context(|| format!("Invalid theme at '{}'", theme_path.display()))?;
                Some(theme)
            } else {
                let theme = if self.light_theme() {
                    decode_bin(LIGHT_THEME).context("Invalid builtin light theme")?
                } else {
                    decode_bin(DARK_THEME).context("Invalid builtin dark theme")?
                };
                Some(theme)
            }
        } else {
            None
        };
        let wrap = if *IS_STDOUT_TERMINAL {
            self.wrap.clone()
        } else {
            None
        };
        let truecolor = matches!(
            env::var("COLORTERM").as_ref().map(|v| v.as_str()),
            Ok("truecolor")
        );
        Ok(RenderOptions::new(theme, wrap, self.wrap_code, truecolor))
    }

    pub fn render_prompt_left(&self) -> String {
        let variables = self.generate_prompt_context();
        let left_prompt = self.left_prompt.as_deref().unwrap_or(LEFT_PROMPT);
        render_prompt(left_prompt, &variables)
    }

    pub fn render_prompt_right(&self) -> String {
        let variables = self.generate_prompt_context();
        let right_prompt = self.right_prompt.as_deref().unwrap_or(RIGHT_PROMPT);
        render_prompt(right_prompt, &variables)
    }

    pub fn print_markdown(&self, text: &str) -> Result<()> {
        if *IS_STDOUT_TERMINAL {
            let render_options = self.render_options()?;
            let mut markdown_render = MarkdownRender::init(render_options)?;
            println!("{}", markdown_render.render(text));
        } else {
            println!("{text}");
        }
        Ok(())
    }

    fn generate_prompt_context(&self) -> HashMap<&str, String> {
        let mut output = HashMap::new();
        let role = self.extract_role();
        output.insert("model", role.model().id());
        output.insert("client_name", role.model().client_name().to_string());
        output.insert("model_name", role.model().name().to_string());
        output.insert(
            "max_input_tokens",
            role.model()
                .max_input_tokens()
                .unwrap_or_default()
                .to_string(),
        );
        if let Some(temperature) = role.temperature() {
            if temperature != 0.0 {
                output.insert("temperature", temperature.to_string());
            }
        }
        if let Some(top_p) = role.top_p() {
            if top_p != 0.0 {
                output.insert("top_p", top_p.to_string());
            }
        }
        if self.dry_run {
            output.insert("dry_run", "true".to_string());
        }
        if self.stream {
            output.insert("stream", "true".to_string());
        }
        if self.save {
            output.insert("save", "true".to_string());
        }
        if let Some(wrap) = &self.wrap {
            if wrap != "no" {
                output.insert("wrap", wrap.clone());
            }
        }
        if !role.is_derived() {
            output.insert("role", role.name().to_string());
        }
        if let Some(session) = &self.session {
            output.insert("session", session.name().to_string());
            if let Some(autoname) = session.autoname() {
                output.insert("session_autoname", autoname.to_string());
            }
            output.insert("dirty", session.dirty().to_string());
            let (tokens, percent) = session.tokens_usage();
            output.insert("consume_tokens", tokens.to_string());
            output.insert("consume_percent", percent.to_string());
            output.insert("user_messages_len", session.user_messages_len().to_string());
        }
        if let Some(rag) = &self.rag {
            output.insert("rag", rag.name().to_string());
        }
        if let Some(agent) = &self.agent {
            output.insert("agent", agent.name().to_string());
        }

        if self.highlight {
            output.insert("color.reset", "\u{1b}[0m".to_string());
            output.insert("color.black", "\u{1b}[30m".to_string());
            output.insert("color.dark_gray", "\u{1b}[90m".to_string());
            output.insert("color.red", "\u{1b}[31m".to_string());
            output.insert("color.light_red", "\u{1b}[91m".to_string());
            output.insert("color.green", "\u{1b}[32m".to_string());
            output.insert("color.light_green", "\u{1b}[92m".to_string());
            output.insert("color.yellow", "\u{1b}[33m".to_string());
            output.insert("color.light_yellow", "\u{1b}[93m".to_string());
            output.insert("color.blue", "\u{1b}[34m".to_string());
            output.insert("color.light_blue", "\u{1b}[94m".to_string());
            output.insert("color.purple", "\u{1b}[35m".to_string());
            output.insert("color.light_purple", "\u{1b}[95m".to_string());
            output.insert("color.magenta", "\u{1b}[35m".to_string());
            output.insert("color.light_magenta", "\u{1b}[95m".to_string());
            output.insert("color.cyan", "\u{1b}[36m".to_string());
            output.insert("color.light_cyan", "\u{1b}[96m".to_string());
            output.insert("color.white", "\u{1b}[37m".to_string());
            output.insert("color.light_gray", "\u{1b}[97m".to_string());
        }

        output
    }

    pub fn before_chat_completion(&mut self, input: &Input) -> Result<()> {
        if let Some(command) = &self.pre_command {
            let mut envs = HashMap::new();
            envs.insert("AICHAT_INPUT", input.raw());
            run_hook(command, &envs)?;
        }
        self.last_message = Some(LastMessage::new(input.clone(), String::new()));
        Ok(())
    }

    pub fn after_chat_completion(
        &mut self,
        input: &Input,
        output: &str,
        tool_results: &[ToolResult],
    ) -> Result<()> {
        if !tool_results.is_empty() {
            return Ok(());
        }
        if let Some(command) = &self.post_command {
            let mut envs = HashMap::new();
            envs.insert("AICHAT_INPUT", input.raw());
            envs.insert("AICHAT_OUTPUT", output.to_string());
            run_hook(command, &envs)?;
        }
        self.last_message = Some(LastMessage::new(input.clone(), output.to_string()));
        if !self.dry_run {
            self.save_message(input, output)?;
        }
        Ok(())
    }

    fn discontinuous_last_message(&mut self) {
        if let Some(last_message) = self.last_message.as_mut() {
            last_message.continuous = false;
        }
    }

    fn save_message(&mut self, input: &Input, output: &str) -> Result<()> {
        let mut input = input.clone();
        input.clear_patch();
        if let Some(session) = input.session_mut(&mut self.session) {
            session.add_message(&input, output)?;
            return Ok(());
        }

        if !self.save {
            return Ok(());
        }
        let mut file = self.open_message_file()?;
        if output.is_empty() && input.tool_calls().is_none() {
            return Ok(());
        }
        let now = now();
        let summary = input.summary();
        let raw_input = input.raw();
        let scope = if self.agent.is_none() {
            let role_name = if input.role().is_derived() {
                None
            } else {
                Some(input.role().name())
            };
            match (role_name, input.rag_name()) {
                (Some(role), Some(rag_name)) => format!(" ({role}#{rag_name})"),
                (Some(role), _) => format!(" ({role})"),
                (None, Some(rag_name)) => format!(" (#{rag_name})"),
                _ => String::new(),
            }
        } else {
            String::new()
        };
        let tool_calls = match input.tool_calls() {
            Some(MessageContentToolCalls {
                tool_results, text, ..
            }) => {
                let mut lines = vec!["<tool_calls>".to_string()];
                if !text.is_empty() {
                    lines.push(text.clone());
                }
                lines.push(serde_json::to_string(&tool_results).unwrap_or_default());
                lines.push("</tool_calls>\n".to_string());
                lines.join("\n")
            }
            None => String::new(),
        };
        let output = format!(
            "# CHAT: {summary} [{now}]{scope}\n{raw_input}\n--------\n{tool_calls}{output}\n--------\n\n",
        );
        file.write_all(output.as_bytes())
            .with_context(|| "Failed to save message")
    }

    fn init_agent_shared_variables(&mut self) -> Result<()> {
        let agent = match self.agent.as_mut() {
            Some(v) => v,
            None => return Ok(()),
        };
        if !agent.defined_variables().is_empty() && agent.shared_variables().is_empty() {
            let mut config_variables = agent.config_variables().clone();
            if let Some(v) = &self.agent_variables {
                config_variables.extend(v.clone());
            }
            let new_variables = Agent::init_agent_variables(
                agent.defined_variables(),
                &config_variables,
                self.info_flag,
            )?;
            agent.set_shared_variables(new_variables);
        }
        if !self.info_flag {
            agent.update_shared_dynamic_instructions(false)?;
        }
        Ok(())
    }

    fn init_agent_session_variables(&mut self, new_session: bool) -> Result<()> {
        let (agent, session) = match (self.agent.as_mut(), self.session.as_mut()) {
            (Some(agent), Some(session)) => (agent, session),
            _ => return Ok(()),
        };
        if new_session {
            let shared_variables = agent.shared_variables().clone();
            let session_variables =
                if !agent.defined_variables().is_empty() && shared_variables.is_empty() {
                    let mut config_variables = agent.config_variables().clone();
                    if let Some(v) = &self.agent_variables {
                        config_variables.extend(v.clone());
                    }
                    let new_variables = Agent::init_agent_variables(
                        agent.defined_variables(),
                        &config_variables,
                        self.info_flag,
                    )?;
                    agent.set_shared_variables(new_variables.clone());
                    new_variables
                } else {
                    shared_variables
                };
            agent.set_session_variables(session_variables);
            if !self.info_flag {
                agent.update_session_dynamic_instructions(None)?;
            }
            session.sync_agent(agent);
        } else {
            let variables = session.agent_variables();
            agent.set_session_variables(variables.clone());
            agent.update_session_dynamic_instructions(Some(
                session.agent_instructions().to_string(),
            ))?;
        }
        Ok(())
    }

    fn open_message_file(&self) -> Result<File> {
        let path = self.messages_file();
        ensure_parent_exists(&path)?;
        OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .with_context(|| format!("Failed to create/append {}", path.display()))
    }

    fn load_from_file(config_path: &Path) -> Result<Self> {
        let err = || format!("Failed to load config at '{}'", config_path.display());
        let content = read_to_string(config_path).with_context(err)?;
        let config: Self = serde_yaml::from_str(&content)
            .map_err(|err| {
                let err_msg = err.to_string();
                let err_msg = if err_msg.starts_with(&format!("{CLIENTS_FIELD}: ")) {
                    // location is incorrect, get rid of it
                    err_msg
                        .split_once(" at line")
                        .map(|(v, _)| {
                            format!("{v} (Sorry for being unable to provide an exact location)")
                        })
                        .unwrap_or_else(|| "clients: invalid value".into())
                } else {
                    err_msg
                };
                anyhow!("{err_msg}")
            })
            .with_context(err)?;

        Ok(config)
    }

    fn load_dynamic(model_id: &str) -> Result<Self> {
        let provider = match model_id.split_once(':') {
            Some((v, _)) => v,
            _ => model_id,
        };
        let is_openai_compatible = OPENAI_COMPATIBLE_PROVIDERS
            .into_iter()
            .any(|(name, _)| provider == name);
        let client = if is_openai_compatible {
            json!({ "type": "openai-compatible", "name": provider })
        } else {
            json!({ "type": provider })
        };
        let config = json!({
            "model": model_id.to_string(),
            "save": false,
            "clients": vec![client],
        });
        let config =
            serde_json::from_value(config).with_context(|| "Failed to load config from env")?;
        Ok(config)
    }

    fn load_envs(&mut self) {
        if let Ok(v) = env::var(get_env_name("model")) {
            self.model_id = v;
        }
        if let Some(v) = read_env_value::<f64>(&get_env_name("temperature")) {
            self.temperature = v;
        }
        if let Some(v) = read_env_value::<f64>(&get_env_name("top_p")) {
            self.top_p = v;
        }

        if let Some(Some(v)) = read_env_bool(&get_env_name("dry_run")) {
            self.dry_run = v;
        }
        if let Some(Some(v)) = read_env_bool(&get_env_name("stream")) {
            self.stream = v;
        }
        if let Some(Some(v)) = read_env_bool(&get_env_name("save")) {
            self.save = v;
        }
        if let Ok(v) = env::var(get_env_name("keybindings")) {
            if v == "vi" {
                self.keybindings = v;
            }
        }
        if let Some(v) = read_env_value::<String>(&get_env_name("editor")) {
            self.editor = v;
        }
        if let Some(v) = read_env_value::<String>(&get_env_name("wrap")) {
            self.wrap = v;
        }
        if let Some(Some(v)) = read_env_bool(&get_env_name("wrap_code")) {
            self.wrap_code = v;
        }

        if let Some(Some(v)) = read_env_bool(&get_env_name("function_calling")) {
            self.function_calling = v;
        }
        if let Ok(v) = env::var(get_env_name("mapping_tools")) {
            if let Ok(v) = serde_json::from_str(&v) {
                self.mapping_tools = v;
            }
        }
        if let Some(v) = read_env_value::<String>(&get_env_name("use_tools")) {
            self.use_tools = v;
        }

        if let Some(v) = read_env_value::<String>(&get_env_name("repl_prelude")) {
            self.repl_prelude = v;
        }
        if let Some(v) = read_env_value::<String>(&get_env_name("cmd_prelude")) {
            self.cmd_prelude = v;
        }
        if let Some(v) = read_env_value::<String>(&get_env_name("agent_prelude")) {
            self.agent_prelude = v;
        }

        if let Some(v) = read_env_value::<String>(&get_env_name("pre_command")) {
            self.pre_command = v;
        }
        if let Some(v) = read_env_value::<String>(&get_env_name("post_command")) {
            self.post_command = v;
        }

        if let Some(v) = read_env_bool(&get_env_name("save_session")) {
            self.save_session = v;
        }
        if let Some(Some(v)) = read_env_value::<usize>(&get_env_name("compress_threshold")) {
            self.compress_threshold = v;
        }
        if let Some(v) = read_env_value::<String>(&get_env_name("summarize_prompt")) {
            self.summarize_prompt = v;
        }
        if let Some(v) = read_env_value::<String>(&get_env_name("summary_prompt")) {
            self.summary_prompt = v;
        }

        if let Some(v) = read_env_value::<String>(&get_env_name("rag_embedding_model")) {
            self.rag_embedding_model = v;
        }
        if let Some(v) = read_env_value::<String>(&get_env_name("rag_reranker_model")) {
            self.rag_reranker_model = v;
        }
        if let Some(Some(v)) = read_env_value::<usize>(&get_env_name("rag_top_k")) {
            self.rag_top_k = v;
        }
        if let Some(v) = read_env_value::<usize>(&get_env_name("rag_chunk_size")) {
            self.rag_chunk_size = v;
        }
        if let Some(v) = read_env_value::<usize>(&get_env_name("rag_chunk_overlap")) {
            self.rag_chunk_overlap = v;
        }
        if let Some(v) = read_env_value::<String>(&get_env_name("rag_template")) {
            self.rag_template = v;
        }

        if let Ok(v) = env::var(get_env_name("document_loaders")) {
            if let Ok(v) = serde_json::from_str(&v) {
                self.document_loaders = v;
            }
        }

        if let Some(Some(v)) = read_env_bool(&get_env_name("highlight")) {
            self.highlight = v;
        }
        if *NO_COLOR {
            self.highlight = false;
        }
        if self.highlight && self.theme.is_none() {
            if let Some(v) = read_env_value::<String>(&get_env_name("theme")) {
                self.theme = v;
            } else if *IS_STDOUT_TERMINAL {
                if let Ok(color_scheme) = color_scheme(QueryOptions::default()) {
                    let theme = match color_scheme {
                        ColorScheme::Dark => "dark",
                        ColorScheme::Light => "light",
                    };
                    self.theme = Some(theme.into());
                }
            }
        }
        if let Some(v) = read_env_value::<String>(&get_env_name("left_prompt")) {
            self.left_prompt = v;
        }
        if let Some(v) = read_env_value::<String>(&get_env_name("right_prompt")) {
            self.right_prompt = v;
        }

        if let Some(v) = read_env_value::<String>(&get_env_name("serve_addr")) {
            self.serve_addr = v;
        }
        if let Some(v) = read_env_value::<String>(&get_env_name("user_agent")) {
            self.user_agent = v;
        }
        if let Some(Some(v)) = read_env_bool(&get_env_name("save_shell_history")) {
            self.save_shell_history = v;
        }
        if let Some(v) = read_env_value::<String>(&get_env_name("sync_models_url")) {
            self.sync_models_url = v;
        }
    }

    fn load_functions(&mut self) -> Result<()> {
        self.functions = Functions::init(&Self::functions_file())?;
        Ok(())
    }

    fn setup_model(&mut self) -> Result<()> {
        let mut model_id = self.model_id.clone();
        if model_id.is_empty() {
            let models = list_models(self, ModelType::Chat);
            if models.is_empty() {
                bail!("No available model");
            }
            model_id = models[0].id()
        };
        self.set_model(&model_id)?;
        self.model_id = model_id;
        Ok(())
    }

    fn setup_document_loaders(&mut self) {
        [("pdf", "pdftotext $1 -"), ("docx", "pandoc --to plain $1")]
            .into_iter()
            .for_each(|(k, v)| {
                let (k, v) = (k.to_string(), v.to_string());
                self.document_loaders.entry(k).or_insert(v);
            });
    }

    fn setup_user_agent(&mut self) {
        if let Some("auto") = self.user_agent.as_deref() {
            self.user_agent = Some(format!(
                "{}/{}",
                env!("CARGO_CRATE_NAME"),
                env!("CARGO_PKG_VERSION")
            ));
        }
    }
}

pub fn load_env_file() -> Result<()> {
    let env_file_path = Config::env_file();
    let contents = match read_to_string(&env_file_path) {
        Ok(v) => v,
        Err(_) => return Ok(()),
    };
    debug!("Use env file '{}'", env_file_path.display());
    for line in contents.lines() {
        let line = line.trim();
        if line.starts_with('#') || line.is_empty() {
            continue;
        }
        if let Some((key, value)) = line.split_once('=') {
            env::set_var(key.trim(), value.trim());
        }
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum WorkingMode {
    Cmd,
    Repl,
    Serve,
}

impl WorkingMode {
    pub fn is_cmd(&self) -> bool {
        *self == WorkingMode::Cmd
    }
    pub fn is_repl(&self) -> bool {
        *self == WorkingMode::Repl
    }
    pub fn is_serve(&self) -> bool {
        *self == WorkingMode::Serve
    }
}

#[async_recursion::async_recursion]
pub async fn macro_execute(
    config: &GlobalConfig,
    name: &str,
    args: Option<&str>,
    abort_signal: AbortSignal,
) -> Result<()> {
    let macro_value = Config::load_macro(name)?;
    let (mut new_args, text) = split_args_text(args.unwrap_or_default(), cfg!(windows));
    if !text.is_empty() {
        new_args.push(text.to_string());
    }
    let variables = macro_value
        .resolve_variables(&new_args)
        .map_err(|err| anyhow!("{err}. Usage: {}", macro_value.usage(name)))?;
    let role = config.read().extract_role();
    let mut config = config.read().clone();
    config.temperature = role.temperature();
    config.top_p = role.top_p();
    config.use_tools = role.use_tools().clone();
    config.macro_flag = true;
    config.model = role.model().clone();
    config.role = None;
    config.session = None;
    config.rag = None;
    config.agent = None;
    config.discontinuous_last_message();
    let config = Arc::new(RwLock::new(config));
    config.write().macro_flag = true;
    for step in &macro_value.steps {
        let command = Macro::interpolate_command(step, &variables);
        println!(">> {}", multiline_text(&command));
        run_repl_command(&config, abort_signal.clone(), &command).await?;
    }
    Ok(())
}

#[derive(Debug, Clone, Deserialize)]
pub struct Macro {
    #[serde(default)]
    pub variables: Vec<MacroVariable>,
    pub steps: Vec<String>,
}

impl Macro {
    pub fn resolve_variables(&self, args: &[String]) -> Result<IndexMap<String, String>> {
        let mut output = IndexMap::new();
        for (i, variable) in self.variables.iter().enumerate() {
            let value = if variable.rest && i == self.variables.len() - 1 {
                if args.len() > i {
                    Some(args[i..].join(" "))
                } else {
                    variable.default.clone()
                }
            } else {
                args.get(i)
                    .map(|v| v.to_string())
                    .or_else(|| variable.default.clone())
            };
            let value =
                value.ok_or_else(|| anyhow!("Missing value for variable '{}'", variable.name))?;
            output.insert(variable.name.clone(), value);
        }
        Ok(output)
    }

    pub fn usage(&self, name: &str) -> String {
        let mut parts = vec![name.to_string()];
        for (i, variable) in self.variables.iter().enumerate() {
            let part = match (
                variable.rest && i == self.variables.len() - 1,
                variable.default.is_some(),
            ) {
                (true, true) => format!("[{}]...", variable.name),
                (true, false) => format!("<{}>...", variable.name),
                (false, true) => format!("[{}]", variable.name),
                (false, false) => format!("<{}>", variable.name),
            };
            parts.push(part);
        }
        parts.join(" ")
    }

    pub fn interpolate_command(command: &str, variables: &IndexMap<String, String>) -> String {
        let mut output = command.to_string();
        for (key, value) in variables {
            output = output.replace(&format!("{{{{{key}}}}}"), value);
        }
        output
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct MacroVariable {
    pub name: String,
    #[serde(default)]
    pub rest: bool,
    pub default: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelsOverride {
    pub version: String,
    pub list: Vec<ProviderModels>,
}

#[derive(Debug, Clone)]
pub struct LastMessage {
    pub input: Input,
    pub output: String,
    pub continuous: bool,
}

impl LastMessage {
    pub fn new(input: Input, output: String) -> Self {
        Self {
            input,
            output,
            continuous: true,
        }
    }
}

bitflags::bitflags! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct StateFlags: u32 {
        const ROLE = 1 << 0;
        const SESSION_EMPTY = 1 << 1;
        const SESSION = 1 << 2;
        const RAG = 1 << 3;
        const AGENT = 1 << 4;
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AssertState {
    True(StateFlags),
    False(StateFlags),
    TrueFalse(StateFlags, StateFlags),
    Equal(StateFlags),
}

impl AssertState {
    pub fn pass() -> Self {
        AssertState::False(StateFlags::empty())
    }

    pub fn bare() -> Self {
        AssertState::Equal(StateFlags::empty())
    }

    pub fn assert(self, flags: StateFlags) -> bool {
        match self {
            AssertState::True(true_flags) => true_flags & flags != StateFlags::empty(),
            AssertState::False(false_flags) => false_flags & flags == StateFlags::empty(),
            AssertState::TrueFalse(true_flags, false_flags) => {
                (true_flags & flags != StateFlags::empty())
                    && (false_flags & flags == StateFlags::empty())
            }
            AssertState::Equal(check_flags) => check_flags == flags,
        }
    }
}

async fn create_config_file(config_path: &Path) -> Result<()> {
    let ans = Confirm::new("No config file, create a new one?")
        .with_default(true)
        .prompt()?;
    if !ans {
        process::exit(0);
    }

    let client = Select::new("API Provider (required):", list_client_types()).prompt()?;

    let mut config = serde_json::json!({});
    let (model, clients_config) = create_client_config(client).await?;
    config["model"] = model.into();
    config[CLIENTS_FIELD] = clients_config;

    let config_data = serde_yaml::to_string(&config).with_context(|| "Failed to create config")?;
    let config_data = format!(
        "# see https://github.com/sigoden/aichat/blob/main/config.example.yaml\n\n{config_data}"
    );

    ensure_parent_exists(config_path)?;
    std::fs::write(config_path, config_data)
        .with_context(|| format!("Failed to write to '{}'", config_path.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::prelude::PermissionsExt;
        let perms = std::fs::Permissions::from_mode(0o600);
        std::fs::set_permissions(config_path, perms)?;
    }

    println!("✓ Saved the config file to '{}'.\n", config_path.display());

    Ok(())
}

pub(crate) fn ensure_parent_exists(path: &Path) -> Result<()> {
    if path.exists() {
        return Ok(());
    }
    let parent = path
        .parent()
        .ok_or_else(|| anyhow!("Failed to write to '{}', No parent path", path.display()))?;
    if !parent.exists() {
        create_dir_all(parent).with_context(|| {
            format!(
                "Failed to write to '{}', Cannot create parent directory",
                path.display()
            )
        })?;
    }
    Ok(())
}

fn read_env_value<T>(key: &str) -> Option<Option<T>>
where
    T: std::str::FromStr,
{
    let value = env::var(key).ok()?;
    let value = parse_value(&value).ok()?;
    Some(value)
}

fn parse_value<T>(value: &str) -> Result<Option<T>>
where
    T: std::str::FromStr,
{
    let value = if value == "null" {
        None
    } else {
        let value = match value.parse() {
            Ok(value) => value,
            Err(_) => bail!("Invalid value '{}'", value),
        };
        Some(value)
    };
    Ok(value)
}

fn read_env_bool(key: &str) -> Option<Option<bool>> {
    let value = env::var(key).ok()?;
    Some(parse_bool(&value))
}

fn complete_bool(value: bool) -> Vec<String> {
    vec![(!value).to_string()]
}

fn complete_option_bool(value: Option<bool>) -> Vec<String> {
    match value {
        Some(true) => vec!["false".to_string(), "null".to_string()],
        Some(false) => vec!["true".to_string(), "null".to_string()],
        None => vec!["true".to_string(), "false".to_string()],
    }
}

fn map_completion_values<T: ToString>(value: Vec<T>) -> Vec<(String, Option<String>)> {
    value.into_iter().map(|v| (v.to_string(), None)).collect()
}

fn update_rag<F>(config: &GlobalConfig, f: F) -> Result<()>
where
    F: FnOnce(&mut Rag) -> Result<()>,
{
    let mut rag = match config.read().rag.clone() {
        Some(v) => v.as_ref().clone(),
        None => bail!("No RAG"),
    };
    f(&mut rag)?;
    config.write().rag = Some(Arc::new(rag));
    Ok(())
}

fn format_option_value<T>(value: &Option<T>) -> String
where
    T: std::fmt::Display,
{
    match value {
        Some(value) => value.to_string(),
        None => "null".to_string(),
    }
}

fn run_hook(command: &str, envs: &HashMap<&str, String>) -> Result<()> {
    let mut parts = command.split_whitespace();
    let cmd = parts.next().ok_or_else(|| anyhow!("Invalid hook command"))?;
    let args: Vec<&str> = parts.collect();
    let mut child = process::Command::new(cmd)
        .args(args)
        .envs(envs)
        .spawn()
        .with_context(|| format!("Failed to run hook '{command}'"))?;
    child.wait()?;
    Ok(())
}
