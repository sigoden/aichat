mod bot;
mod input;
mod role;
mod session;

pub use self::bot::{list_bots, Bot, BotConfig};
pub use self::input::Input;
pub use self::role::{Role, RoleLike, CODE_ROLE, EXPLAIN_SHELL_ROLE, SHELL_ROLE};
use self::session::Session;

use crate::client::{
    create_client_config, list_chat_models, list_client_types, ClientConfig, Model,
    OPENAI_COMPATIBLE_PLATFORMS,
};
use crate::function::{FunctionDeclaration, Functions, ToolCallResult};
use crate::rag::Rag;
use crate::render::{MarkdownRender, RenderOptions};
use crate::utils::{
    format_option_value, fuzzy_match, get_env_name, light_theme_from_colorfgbg, now, render_prompt,
    set_text, warning_text, AbortSignal, IS_STDOUT_TERMINAL,
};

use anyhow::{anyhow, bail, Context, Result};
use inquire::{Confirm, Select};
use parking_lot::RwLock;
use serde::Deserialize;
use serde_json::json;
use std::collections::{HashMap, HashSet};
use std::{
    env,
    fs::{create_dir_all, read_dir, read_to_string, remove_file, File, OpenOptions},
    io::Write,
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
const RAGS_DIR_NAME: &str = "rags";
const FUNCTIONS_DIR_NAME: &str = "functions";
const FUNCTIONS_FILE_NAME: &str = "functions.json";
const BOTS_DIR_NAME: &str = "bots";
const BOT_DEFINITION_FILE_NAME: &str = "index.yaml";
const BOT_EMBEDDINGS_DIR: &str = "embeddings";
const BOT_RAG_FILE_NAME: &str = "rag.bin";

pub const TEMP_ROLE_NAME: &str = "%%";
pub const TEMP_RAG_NAME: &str = "temp";
pub const TEMP_SESSION_NAME: &str = "temp";

const CLIENTS_FIELD: &str = "clients";

const SUMMARIZE_PROMPT: &str =
    "Summarize the discussion briefly in 200 words or less to use as a prompt for future context.";
const SUMMARY_PROMPT: &str = "This is a summary of the chat history as a recap: ";

const RAG_TEMPLATE: &str = r#"Answer the question based only on the provided context:
<context>
__CONTEXT__
</context>

Question: __INPUT__
"#;

const LEFT_PROMPT: &str = "{color.green}{?session {?bot {bot}#}{session}{?role /}}{!session {?bot {bot}}}{role}{?rag @{rag}}{color.cyan}{?session )}{!session >}{color.reset} ";
const RIGHT_PROMPT: &str = "{color.purple}{?session {?consume_tokens {consume_tokens}({consume_percent}%)}{!consume_tokens {consume_tokens}}}{color.reset}";

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct Config {
    #[serde(rename(serialize = "model", deserialize = "model"))]
    #[serde(default)]
    pub model_id: String,
    pub temperature: Option<f64>,
    pub top_p: Option<f64>,
    pub dry_run: bool,
    pub save: bool,
    pub save_session: Option<bool>,
    pub highlight: bool,
    pub light_theme: bool,
    pub wrap: Option<String>,
    pub wrap_code: bool,
    pub auto_copy: bool,
    pub keybindings: Keybindings,
    pub prelude: Option<String>,
    pub buffer_editor: Option<String>,
    pub embedding_model: Option<String>,
    pub rag_top_k: usize,
    pub rag_template: Option<String>,
    pub function_calling: bool,
    pub compress_threshold: usize,
    pub summarize_prompt: Option<String>,
    pub summary_prompt: Option<String>,
    pub left_prompt: Option<String>,
    pub right_prompt: Option<String>,
    pub clients: Vec<ClientConfig>,
    pub bots: Vec<BotConfig>,
    #[serde(skip)]
    pub roles: Vec<Role>,
    #[serde(skip)]
    pub role: Option<Role>,
    #[serde(skip)]
    pub session: Option<Session>,
    #[serde(skip)]
    pub rag: Option<Arc<Rag>>,
    #[serde(skip)]
    pub bot: Option<Bot>,
    #[serde(skip)]
    pub model: Model,
    #[serde(skip)]
    pub functions: Functions,
    #[serde(skip)]
    pub working_mode: WorkingMode,
    #[serde(skip)]
    pub last_message: Option<(Input, String)>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            model_id: Default::default(),
            temperature: None,
            top_p: None,
            save: false,
            save_session: None,
            highlight: true,
            dry_run: false,
            light_theme: false,
            wrap: None,
            wrap_code: false,
            auto_copy: false,
            keybindings: Default::default(),
            prelude: None,
            buffer_editor: None,
            embedding_model: None,
            rag_top_k: 4,
            rag_template: None,
            function_calling: false,
            compress_threshold: 4000,
            summarize_prompt: None,
            summary_prompt: None,
            left_prompt: None,
            right_prompt: None,
            clients: vec![],
            bots: vec![],
            roles: vec![],
            role: None,
            session: None,
            rag: None,
            bot: None,
            model: Default::default(),
            functions: Default::default(),
            working_mode: WorkingMode::Command,
            last_message: None,
        }
    }
}

pub type GlobalConfig = Arc<RwLock<Config>>;

impl Config {
    pub fn init(working_mode: WorkingMode) -> Result<Self> {
        let config_path = Self::config_file()?;

        let platform = env::var(get_env_name("platform")).ok();
        if *IS_STDOUT_TERMINAL && platform.is_none() && !config_path.exists() {
            create_config_file(&config_path)?;
        }
        let mut config = if platform.is_some() {
            Self::load_config_env(&platform.unwrap())?
        } else {
            Self::load_config_file(&config_path)?
        };

        if let Some(wrap) = config.wrap.clone() {
            config.set_wrap(&wrap)?;
        }

        config.functions = Functions::init(&Self::functions_file()?)?;

        config.working_mode = working_mode;
        config.load_roles()?;

        config.setup_model()?;
        config.setup_highlight();
        config.setup_light_theme()?;

        Ok(config)
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

    pub fn save_message(
        &mut self,
        input: &mut Input,
        output: &str,
        tool_call_results: &[ToolCallResult],
    ) -> Result<()> {
        input.clear_patch_text();
        self.last_message = Some((input.clone(), output.to_string()));

        if self.dry_run || output.is_empty() || !tool_call_results.is_empty() {
            return Ok(());
        }

        if let Some(session) = input.session_mut(&mut self.session) {
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
        let summary = input.summary();
        let input_markdown = input.render();
        let scope = if self.bot.is_none() {
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
        let output = format!("# CHAT: {summary} [{timestamp}]{scope}\n{input_markdown}\n--------\n{output}\n--------\n\n",);
        file.write_all(output.as_bytes())
            .with_context(|| "Failed to save message")
    }

    pub fn maybe_copy(&self, text: &str) {
        if self.auto_copy {
            let _ = set_text(text);
        }
    }

    pub fn config_file() -> Result<PathBuf> {
        match env::var(get_env_name("config_file")) {
            Ok(value) => Ok(PathBuf::from(value)),
            Err(_) => Self::local_path(CONFIG_FILE_NAME),
        }
    }

    pub fn roles_file() -> Result<PathBuf> {
        match env::var(get_env_name("roles_file")) {
            Ok(value) => Ok(PathBuf::from(value)),
            Err(_) => Self::local_path(ROLES_FILE_NAME),
        }
    }

    pub fn messages_file(&self) -> Result<PathBuf> {
        match &self.bot {
            None => match env::var(get_env_name("messages_file")) {
                Ok(value) => Ok(PathBuf::from(value)),
                Err(_) => Self::local_path(MESSAGES_FILE_NAME),
            },
            Some(bot) => Ok(Self::bot_config_dir(bot.name())?.join(MESSAGES_FILE_NAME)),
        }
    }

    pub fn sessions_dir(&self) -> Result<PathBuf> {
        match &self.bot {
            None => match env::var(get_env_name("sessions_dir")) {
                Ok(value) => Ok(PathBuf::from(value)),
                Err(_) => Self::local_path(SESSIONS_DIR_NAME),
            },
            Some(bot) => Ok(Self::bot_config_dir(bot.name())?.join(SESSIONS_DIR_NAME)),
        }
    }

    pub fn rags_dir() -> Result<PathBuf> {
        match env::var(get_env_name("rags_dir")) {
            Ok(value) => Ok(PathBuf::from(value)),
            Err(_) => Self::local_path(RAGS_DIR_NAME),
        }
    }

    pub fn functions_dir() -> Result<PathBuf> {
        match env::var(get_env_name("functions_dir")) {
            Ok(value) => Ok(PathBuf::from(value)),
            Err(_) => Self::local_path(FUNCTIONS_DIR_NAME),
        }
    }

    pub fn functions_file() -> Result<PathBuf> {
        Ok(Self::functions_dir()?.join(FUNCTIONS_FILE_NAME))
    }

    pub fn functions_bin_dir() -> Result<PathBuf> {
        Ok(Self::functions_dir()?.join("bin"))
    }

    pub fn session_file(&self, name: &str) -> Result<PathBuf> {
        Ok(self.sessions_dir()?.join(format!("{name}.yaml")))
    }

    pub fn rag_file(&self, name: &str) -> Result<PathBuf> {
        let path = if self.bot.is_none() {
            Self::rags_dir()?.join(format!("{name}.bin"))
        } else {
            Self::rags_dir()?
                .join(BOTS_DIR_NAME)
                .join(format!("{name}.bin"))
        };
        Ok(path)
    }

    pub fn bots_dir() -> Result<PathBuf> {
        match env::var(get_env_name("bots_config_dir")) {
            Ok(value) => Ok(PathBuf::from(value)),
            Err(_) => Self::local_path(BOTS_DIR_NAME),
        }
    }

    pub fn bot_config_dir(name: &str) -> Result<PathBuf> {
        Ok(Self::bots_dir()?.join(name))
    }

    pub fn bot_rag_file(name: &str) -> Result<PathBuf> {
        Ok(Self::bot_config_dir(name)?.join(BOT_RAG_FILE_NAME))
    }

    pub fn bots_functions_dir() -> Result<PathBuf> {
        Ok(Self::functions_dir()?.join(BOTS_DIR_NAME))
    }

    pub fn bot_functions_dir(name: &str) -> Result<PathBuf> {
        Ok(Self::bots_functions_dir()?.join(name))
    }

    pub fn bot_functions_file(name: &str) -> Result<PathBuf> {
        Ok(Self::bot_functions_dir(name)?.join(FUNCTIONS_FILE_NAME))
    }

    pub fn bot_definition_file(name: &str) -> Result<PathBuf> {
        Ok(Self::bot_functions_dir(name)?.join(BOT_DEFINITION_FILE_NAME))
    }

    pub fn bot_embeddings_dir(name: &str) -> Result<PathBuf> {
        Ok(Self::bot_functions_dir(name)?.join(BOT_EMBEDDINGS_DIR))
    }

    pub fn state(&self) -> StateFlags {
        let mut flags = StateFlags::empty();
        if let Some(session) = &self.session {
            if session.is_empty() {
                flags |= StateFlags::SESSION_EMPTY;
            } else {
                flags |= StateFlags::SESSION;
            }
        }
        if self.bot.is_some() {
            flags |= StateFlags::BOT;
        }
        if self.role.is_some() {
            flags |= StateFlags::ROLE;
        }
        if self.rag.is_some() {
            flags |= StateFlags::RAG;
        }
        flags
    }

    pub fn info(&self) -> Result<String> {
        if let Some(session) = &self.session {
            session.export()
        } else if let Some(role) = &self.role {
            role.export()
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
        let role = self.extract_role();
        let items = vec![
            ("model", role.model().id()),
            (
                "max_output_tokens",
                self.model
                    .max_tokens_param()
                    .map(|v| format!("{v} (current model)"))
                    .unwrap_or_else(|| "-".into()),
            ),
            ("temperature", format_option_value(&role.temperature())),
            ("top_p", format_option_value(&role.top_p())),
            ("rag_top_k", self.rag_top_k.to_string()),
            ("function_calling", self.function_calling.to_string()),
            ("compress_threshold", self.compress_threshold.to_string()),
            ("dry_run", self.dry_run.to_string()),
            ("save", self.save.to_string()),
            ("save_session", format_option_value(&self.save_session)),
            ("highlight", self.highlight.to_string()),
            ("light_theme", self.light_theme.to_string()),
            ("wrap", wrap),
            ("wrap_code", self.wrap_code.to_string()),
            ("auto_copy", self.auto_copy.to_string()),
            ("keybindings", self.keybindings.stringify().into()),
            ("prelude", format_option_value(&self.prelude)),
            ("config_file", display_path(&Self::config_file()?)),
            ("roles_file", display_path(&Self::roles_file()?)),
            ("functions_dir", display_path(&Self::functions_dir()?)),
            ("rags_dir", display_path(&Self::rags_dir()?)),
            ("bots_dir", display_path(&Self::bots_dir()?)),
            ("sessions_dir", display_path(&self.sessions_dir()?)),
            ("messages_file", display_path(&self.messages_file()?)),
        ];
        let output = items
            .iter()
            .map(|(name, value)| format!("{name:<20}{value}"))
            .collect::<Vec<String>>()
            .join("\n");
        Ok(output)
    }

    pub fn update(&mut self, data: &str) -> Result<()> {
        let parts: Vec<&str> = data.split_whitespace().collect();
        if parts.len() != 2 {
            bail!("Usage: .set <key> <value>. If value is null, unset key.");
        }
        let key = parts[0];
        let value = parts[1];
        match key {
            "max_output_tokens" => {
                let value = parse_value(value)?;
                self.model.set_max_tokens(value, true);
            }
            "temperature" => {
                let value = parse_value(value)?;
                self.set_temperature(value);
            }
            "top_p" => {
                let value = parse_value(value)?;
                self.set_top_p(value);
            }
            "rag_top_k" => {
                if let Some(value) = parse_value(value)? {
                    self.rag_top_k = value;
                }
            }
            "function_calling" => {
                let value = value.parse().with_context(|| "Invalid value")?;
                self.function_calling = value;
            }
            "compress_threshold" => {
                let value = parse_value(value)?;
                self.set_compress_threshold(value);
            }
            "save" => {
                let value = value.parse().with_context(|| "Invalid value")?;
                self.save = value;
            }
            "save_session" => {
                let value = parse_value(value)?;
                self.set_save_session(value);
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

    pub fn set_model(&mut self, model_id: &str) -> Result<()> {
        let model = Model::retrieve(self, model_id)?;
        match self.role_like_mut() {
            Some(role_like) => role_like.set_model(&model),
            None => {
                self.model = model;
            }
        }
        Ok(())
    }

    pub fn use_prompt(&mut self, prompt: &str) -> Result<()> {
        let role = Role::new(TEMP_ROLE_NAME, prompt);
        self.use_role_obj(role)
    }

    pub fn use_role(&mut self, name: &str) -> Result<()> {
        let role = self.retrieve_role(name)?;
        self.use_role_obj(role)
    }

    pub fn use_role_obj(&mut self, role: Role) -> Result<()> {
        if self.bot.is_some() {
            bail!("Cannot perform this action because you are using a bot")
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
        if let Some(role) = &self.role {
            role.export()
        } else {
            bail!("No role")
        }
    }

    pub fn exit_role(&mut self) -> Result<()> {
        if self.role.is_some() {
            if let Some(session) = self.session.as_mut() {
                session.clear_role();
            }
            self.role = None;
        }
        Ok(())
    }

    pub fn retrieve_role(&self, name: &str) -> Result<Role> {
        let mut role = self
            .roles
            .iter()
            .find(|v| v.match_name(name))
            .map(|v| {
                let mut role = v.clone();
                role.complete_prompt_args(name);
                role
            })
            .ok_or_else(|| anyhow!("Unknown role `{name}`"))?;

        match role.model_id() {
            Some(model_id) => {
                if self.model.id() != model_id {
                    let model = Model::retrieve(self, model_id)?;
                    role.set_model(&model);
                }
            }
            None => role.set_model(&self.model),
        }
        Ok(role)
    }

    pub fn use_session(&mut self, session: Option<&str>) -> Result<()> {
        if self.session.is_some() {
            bail!(
                "Already in a session, please run '.exit session' first to exit the current session."
            );
        }
        match session {
            None => {
                let session_file = self.session_file(TEMP_SESSION_NAME)?;
                if session_file.exists() {
                    remove_file(session_file).with_context(|| {
                        format!("Failed to cleanup previous '{TEMP_SESSION_NAME}' session")
                    })?;
                }
                let session = Session::new(self, TEMP_SESSION_NAME);
                self.session = Some(session);
            }
            Some(name) => {
                let session_path = self.session_file(name)?;
                if !session_path.exists() {
                    self.session = Some(Session::new(self, name));
                } else {
                    let session = Session::load(self, name, &session_path)?;
                    self.session = Some(session);
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
                    .prompt()?;
                    if ans {
                        session.add_message(input, output)?;
                    }
                }
            }
        }
        Ok(())
    }

    pub fn session_info(&self) -> Result<String> {
        if let Some(session) = &self.session {
            let render_options = self.render_options()?;
            let mut markdown_render = MarkdownRender::init(render_options)?;
            session.render(&mut markdown_render)
        } else {
            bail!("No session")
        }
    }

    pub fn exit_session(&mut self) -> Result<()> {
        if let Some(mut session) = self.session.take() {
            let is_repl = self.working_mode == WorkingMode::Repl;
            let sessions_dir = self.sessions_dir()?;
            session.exit(&sessions_dir, is_repl)?;
            self.last_message = None;
        }
        Ok(())
    }

    pub fn save_session(&mut self, name: &str) -> Result<()> {
        let sessions_dir = self.sessions_dir()?;
        if let Some(session) = self.session.as_mut() {
            if !name.is_empty() {
                session.set_name(name);
            }
            session.save(&sessions_dir)?;
        }
        Ok(())
    }

    pub fn clear_session_messages(&mut self) -> Result<()> {
        if let Some(session) = self.session.as_mut() {
            session.clear_messages();
        }
        Ok(())
    }

    pub fn list_sessions(&self) -> Vec<String> {
        let sessions_dir = match self.sessions_dir() {
            Ok(dir) => dir,
            Err(_) => return vec![],
        };
        match read_dir(sessions_dir) {
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

    pub fn should_compress_session(&mut self) -> bool {
        if let Some(session) = self.session.as_mut() {
            if session.need_compress(self.compress_threshold) {
                session.set_compressing(true);
                return true;
            }
        }
        false
    }

    pub fn compress_session(&mut self, summary: &str) {
        if let Some(session) = self.session.as_mut() {
            let summary_prompt = self.summary_prompt.as_deref().unwrap_or(SUMMARY_PROMPT);
            session.compress(format!("{}{}", summary_prompt, summary));
        }
    }

    pub fn summarize_prompt(&self) -> &str {
        self.summarize_prompt.as_deref().unwrap_or(SUMMARIZE_PROMPT)
    }

    pub fn is_compressing_session(&self) -> bool {
        self.session
            .as_ref()
            .map(|v| v.compressing())
            .unwrap_or_default()
    }

    pub fn end_compressing_session(&mut self) {
        if let Some(session) = self.session.as_mut() {
            session.set_compressing(false);
        }
    }

    pub async fn use_rag(
        config: &GlobalConfig,
        rag: Option<&str>,
        abort_signal: AbortSignal,
    ) -> Result<()> {
        if config.read().bot.is_some() {
            bail!("Cannot perform this action because you are using a bot")
        }
        let rag = match rag {
            None => {
                let rag_path = config.read().rag_file(TEMP_RAG_NAME)?;
                if rag_path.exists() {
                    remove_file(&rag_path).with_context(|| {
                        format!("Failed to cleanup previous '{TEMP_RAG_NAME}' rag")
                    })?;
                }
                Rag::init(config, TEMP_RAG_NAME, &rag_path, &[], abort_signal).await?
            }
            Some(name) => {
                let rag_path = config.read().rag_file(name)?;
                if !rag_path.exists() {
                    Rag::init(config, name, &rag_path, &[], abort_signal).await?
                } else {
                    Rag::load(config, name, &rag_path)?
                }
            }
        };
        config.write().rag = Some(Arc::new(rag));
        Ok(())
    }

    pub fn rag_info(&self) -> Result<String> {
        if let Some(rag) = &self.rag {
            rag.export()
        } else {
            bail!("No rag")
        }
    }

    pub fn exit_rag(&mut self) -> Result<()> {
        self.rag.take();
        Ok(())
    }

    pub fn list_rags(&self) -> Vec<String> {
        let rags_dir = match Self::rags_dir() {
            Ok(dir) => dir,
            Err(_) => return vec![],
        };
        match read_dir(rags_dir) {
            Ok(rd) => {
                let mut names = vec![];
                for entry in rd.flatten() {
                    let name = entry.file_name();
                    if let Some(name) = name.to_string_lossy().strip_suffix(".bin") {
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

    pub async fn use_bot(
        config: &GlobalConfig,
        name: &str,
        abort_signal: AbortSignal,
    ) -> Result<()> {
        if !config.read().function_calling {
            bail!("Before using the bot, please configure function calling first.");
        }
        if config.read().bot.is_some() {
            bail!("Already in a bot, please run '.exit bot' first to exit the current bot.");
        }
        let bot = Bot::init(config, name, abort_signal).await?;
        config.write().rag = bot.rag();
        config.write().bot = Some(bot);
        Ok(())
    }

    pub fn bot_info(&self) -> Result<String> {
        if let Some(bot) = &self.bot {
            bot.export()
        } else {
            bail!("No rag")
        }
    }

    pub fn exit_bot(&mut self) -> Result<()> {
        self.rag.take();
        self.bot.take();
        Ok(())
    }

    pub fn current_model(&self) -> &Model {
        if let Some(session) = self.session.as_ref() {
            session.model()
        } else if let Some(bot) = self.bot.as_ref() {
            bot.model()
        } else if let Some(role) = self.role.as_ref() {
            role.model()
        } else {
            &self.model
        }
    }

    pub fn role_like_mut(&mut self) -> Option<&mut dyn RoleLike> {
        if let Some(session) = self.session.as_mut() {
            Some(session)
        } else if let Some(bot) = self.bot.as_mut() {
            Some(bot)
        } else if let Some(role) = self.role.as_mut() {
            Some(role)
        } else {
            None
        }
    }

    pub fn extract_role(&self) -> Role {
        let mut role = if let Some(session) = self.session.as_ref() {
            session.to_role()
        } else if let Some(bot) = self.bot.as_ref() {
            bot.to_role()
        } else if let Some(role) = self.role.as_ref() {
            role.clone()
        } else {
            let mut role = Role::default();
            role.batch_set(&self.model, self.temperature, self.top_p, None);
            role
        };
        if role.temperature().is_none() && self.temperature.is_some() {
            role.set_temperature(self.temperature);
        }
        if role.top_p().is_none() && self.top_p.is_some() {
            role.set_top_p(self.top_p);
        }
        role
    }

    pub fn apply_prelude(&mut self) -> Result<()> {
        let prelude = self.prelude.clone().unwrap_or_default();
        if prelude.is_empty() {
            return Ok(());
        }
        let err_msg = || format!("Invalid prelude '{}", prelude);
        match prelude.split_once(':') {
            Some(("role", name)) => {
                if self.role.is_none() && self.session.is_none() {
                    self.use_role(name).with_context(err_msg)?;
                }
            }
            Some(("session", name)) => {
                if self.session.is_none() {
                    self.use_session(Some(name)).with_context(err_msg)?;
                }
            }
            _ => {
                bail!("{}", err_msg())
            }
        }
        Ok(())
    }

    pub fn retrieve_functions(
        &self,
        model: &Model,
        role: &Role,
    ) -> Option<Vec<FunctionDeclaration>> {
        let mut functions = None;
        if self.function_calling {
            let function_matcher = role.function_matcher();
            if let Some(matcher) = function_matcher {
                functions = match &self.bot {
                    Some(bot) => bot.functions().select(&matcher),
                    None => self.functions.select(&matcher),
                };
                if !model.supports_function_calling() {
                    functions = None;
                    if *IS_STDOUT_TERMINAL {
                        eprintln!("{}", warning_text("WARNING: the role or session includes functions, but the model or client does not support function calling."));
                    }
                }
            }
        };
        functions
    }

    pub fn buffer_editor(&self) -> Option<String> {
        self.buffer_editor
            .clone()
            .or_else(|| env::var("VISUAL").ok().or_else(|| env::var("EDITOR").ok()))
    }

    pub fn repl_complete(&self, cmd: &str, args: &[&str]) -> Vec<(String, Option<String>)> {
        let (values, filter) = if args.len() == 1 {
            let values = match cmd {
                ".role" => self
                    .roles
                    .iter()
                    .map(|v| (v.name().to_string(), None))
                    .collect(),
                ".model" => list_chat_models(self)
                    .into_iter()
                    .map(|v| (v.id(), Some(v.description())))
                    .collect(),
                ".session" => self
                    .list_sessions()
                    .into_iter()
                    .map(|v| (v, None))
                    .collect(),
                ".rag" => self.list_rags().into_iter().map(|v| (v, None)).collect(),
                ".bot" => list_bots().into_iter().map(|v| (v, None)).collect(),
                ".set" => vec![
                    "max_output_tokens",
                    "temperature",
                    "top_p",
                    "rag_top_k",
                    "function_calling",
                    "compress_threshold",
                    "save",
                    "save_session",
                    "highlight",
                    "dry_run",
                    "auto_copy",
                ]
                .into_iter()
                .map(|v| (format!("{v} "), None))
                .collect(),
                _ => vec![],
            };
            (values, args[0])
        } else if args.len() == 2 {
            let values = match args[0] {
                "max_output_tokens" => match self.model.max_output_tokens() {
                    Some(v) => vec![v.to_string()],
                    None => vec![],
                },
                "function_calling" => complete_bool(self.function_calling),
                "save" => complete_bool(self.save),
                "save_session" => {
                    let save_session = if let Some(session) = &self.session {
                        session.save_session()
                    } else {
                        self.save_session
                    };
                    complete_option_bool(save_session)
                }
                "highlight" => complete_bool(self.highlight),
                "dry_run" => complete_bool(self.dry_run),
                "auto_copy" => complete_bool(self.auto_copy),
                _ => vec![],
            };
            (values.into_iter().map(|v| (v, None)).collect(), args[1])
        } else {
            return vec![];
        };
        values
            .into_iter()
            .filter(|(value, _)| fuzzy_match(value, filter))
            .collect()
    }

    pub fn last_reply(&self) -> &str {
        self.last_message
            .as_ref()
            .map(|(_, reply)| reply.as_str())
            .unwrap_or_default()
    }

    pub fn render_options(&self) -> Result<RenderOptions> {
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
        if self.save {
            output.insert("save", "true".to_string());
        }
        if let Some(wrap) = &self.wrap {
            if wrap != "no" {
                output.insert("wrap", wrap.clone());
            }
        }
        if self.auto_copy {
            output.insert("auto_copy", "true".to_string());
        }
        if !role.is_derived() {
            output.insert("role", role.name().to_string());
        }
        if let Some(session) = &self.session {
            output.insert("session", session.name().to_string());
            output.insert("dirty", session.dirty().to_string());
            let (tokens, percent) = session.tokens_usage();
            output.insert("consume_tokens", tokens.to_string());
            output.insert("consume_percent", percent.to_string());
            output.insert("user_messages_len", session.user_messages_len().to_string());
        }
        if let Some(rag) = &self.rag {
            output.insert("rag", rag.name().to_string());
        }
        if let Some(bot) = &self.bot {
            output.insert("bot", bot.name().to_string());
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

    fn open_message_file(&self) -> Result<File> {
        let path = self.messages_file()?;
        ensure_parent_exists(&path)?;
        OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .with_context(|| format!("Failed to create/append {}", path.display()))
    }

    fn load_config_file(config_path: &Path) -> Result<Self> {
        let content = read_to_string(config_path)
            .with_context(|| format!("Failed to load config at {}", config_path.display()))?;
        let config: Self = serde_yaml::from_str(&content).map_err(|err| {
            let err_msg = err.to_string();
            let err_msg = if err_msg.starts_with(&format!("{}: ", CLIENTS_FIELD)) {
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
        })?;

        Ok(config)
    }

    fn load_config_env(platform: &str) -> Result<Self> {
        let model_id = match env::var(get_env_name("model_name")) {
            Ok(model_name) => format!("{platform}:{model_name}"),
            Err(_) => platform.to_string(),
        };
        let is_openai_compatible = OPENAI_COMPATIBLE_PLATFORMS
            .into_iter()
            .any(|(name, _)| platform == name);
        let client = if is_openai_compatible {
            json!({ "type": "openai-compatible", "name": platform })
        } else {
            json!({ "type": platform })
        };
        let config = json!({
            "model": model_id,
            "save": false,
            "clients": vec![client],
        });
        let config =
            serde_json::from_value(config).with_context(|| "Failed to load config from env")?;
        Ok(config)
    }

    fn load_roles(&mut self) -> Result<()> {
        let path = Self::roles_file()?;
        self.roles = if !path.exists() {
            vec![]
        } else {
            let content = read_to_string(&path)
                .with_context(|| format!("Failed to load roles at {}", path.display()))?;
            serde_yaml::from_str(&content).with_context(|| "Invalid roles config")?
        };
        let exist_roles: HashSet<_> = self.roles.iter().map(|v| v.name().to_string()).collect();
        let builtin_roles = Role::builtin();
        for role in builtin_roles {
            if !exist_roles.contains(role.name()) {
                self.roles.push(role);
            }
        }
        Ok(())
    }

    fn setup_model(&mut self) -> Result<()> {
        let model_id = if self.model_id.is_empty() {
            let models = list_chat_models(self);
            if models.is_empty() {
                bail!("No available model");
            }

            models[0].id()
        } else {
            self.model_id.clone()
        };
        self.set_model(&model_id)?;
        self.model_id = model_id;
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum WorkingMode {
    Command,
    Repl,
    Serve,
}

bitflags::bitflags! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct StateFlags: u32 {
        const ROLE = 1 << 0;
        const SESSION_EMPTY = 1 << 1;
        const SESSION = 1 << 2;
        const RAG = 1 << 3;
        const BOT = 1 << 4;
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
}

fn create_config_file(config_path: &Path) -> Result<()> {
    let ans = Confirm::new("No config file, create a new one?")
        .with_default(true)
        .prompt()?;
    if !ans {
        exit(0);
    }

    let client = Select::new("Platform:", list_client_types()).prompt()?;

    let mut config = serde_json::json!({});
    let (model, clients_config) = create_client_config(client)?;
    config["model"] = model.into();
    config[CLIENTS_FIELD] = clients_config;

    let config_data = serde_yaml::to_string(&config).with_context(|| "Failed to create config")?;

    ensure_parent_exists(config_path)?;
    std::fs::write(config_path, config_data).with_context(|| "Failed to write to config file")?;
    #[cfg(unix)]
    {
        use std::os::unix::prelude::PermissionsExt;
        let perms = std::fs::Permissions::from_mode(0o600);
        std::fs::set_permissions(config_path, perms)?;
    }

    println!("✨ Saved config file to '{}'\n", config_path.display());

    Ok(())
}

pub(crate) fn ensure_parent_exists(path: &Path) -> Result<()> {
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
