use std::{
    env,
    fs::{create_dir_all, read_to_string, File, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    process::exit,
};

use anyhow::{anyhow, Result};
use inquire::{Confirm, Text};
use serde::Deserialize;

const CONFIG_FILE_NAME: &str = "config.yaml";
const ROLES_FILE_NAME: &str = "roles.yaml";
const HISTORY_FILE_NAME: &str = "history.txt";
const MESSAGE_FILE_NAME: &str = "messages.md";

#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    /// Openai api key
    pub api_key: String,
    /// What sampling temperature to use, between 0 and 2
    pub temperature: Option<f64>,
    /// Whether to persistently save chat messages
    #[serde(default)]
    pub save: bool,
    /// Whether to highlight reply message
    #[serde(default)]
    pub highlight: bool,
    /// Set proxy
    pub proxy: Option<String>,
    /// Used only for debugging
    #[serde(default)]
    pub dry_run: bool,
    /// Predefined roles
    #[serde(default, skip_serializing)]
    pub roles: Vec<Role>,
}

impl Config {
    pub fn init(is_interactive: bool) -> Result<Config> {
        let config_path = Config::config_file()?;
        if is_interactive && !config_path.exists() {
            create_config_file(&config_path)?;
        }
        let content = read_to_string(&config_path)
            .map_err(|err| anyhow!("Failed to load config at {}, {err}", config_path.display()))?;
        let mut config: Config =
            serde_yaml::from_str(&content).map_err(|err| anyhow!("Invalid config, {err}"))?;
        config.load_roles()?;
        Ok(config)
    }

    pub fn find_role(&self, name: &str) -> Option<Role> {
        self.roles.iter().find(|v| v.name == name).cloned()
    }

    pub fn local_file(name: &str) -> Result<PathBuf> {
        let env_name = format!(
            "{}_CONFIG_DIR",
            env!("CARGO_CRATE_NAME").to_ascii_uppercase()
        );
        let mut path = match env::var(env_name) {
            Ok(v) => PathBuf::from(v),
            Err(_) => dirs::config_dir().ok_or_else(|| anyhow!("Not found config dir"))?,
        };
        path.push(env!("CARGO_CRATE_NAME"));
        if !path.exists() {
            create_dir_all(&path).map_err(|err| {
                anyhow!("Failed to create config dir at {}, {err}", path.display())
            })?;
        }
        path.push(name);
        Ok(path)
    }

    pub fn open_message_file(&self) -> Result<Option<File>> {
        if !self.save {
            return Ok(None);
        }
        let path = Config::messages_file()?;
        let file: Option<File> = if self.save {
            let file = OpenOptions::new()
                .create(true)
                .append(true)
                .open(&path)
                .map_err(|err| anyhow!("Failed to create/append {}, {err}", path.display()))?;
            Some(file)
        } else {
            None
        };
        Ok(file)
    }

    pub fn save_message(file: Option<&mut File>, input: &str, output: &str) {
        if let (false, Some(file)) = (output.is_empty(), file) {
            let _ = file.write_all(
                format!(
                    "AICHAT: {}\n\n--------\n{}\n--------\n\n",
                    input.trim(),
                    output.trim(),
                )
                .as_bytes(),
            );
        }
    }

    pub fn config_file() -> Result<PathBuf> {
        Self::local_file(CONFIG_FILE_NAME)
    }

    pub fn roles_file() -> Result<PathBuf> {
        Self::local_file(ROLES_FILE_NAME)
    }

    pub fn history_file() -> Result<PathBuf> {
        Self::local_file(HISTORY_FILE_NAME)
    }

    fn messages_file() -> Result<PathBuf> {
        Self::local_file(MESSAGE_FILE_NAME)
    }

    fn load_roles(&mut self) -> Result<()> {
        let path = Self::roles_file()?;
        if !path.exists() {
            return Ok(());
        }
        let content = read_to_string(&path)
            .map_err(|err| anyhow!("Failed to load roles at {}, {err}", path.display()))?;
        let roles: Vec<Role> =
            serde_yaml::from_str(&content).map_err(|err| anyhow!("Invalid roles config, {err}"))?;
        self.roles = roles;
        Ok(())
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct Role {
    /// Role name
    pub name: String,
    /// Prompt text send to ai for setting up a role
    pub prompt: String,
}

fn create_config_file(config_path: &Path) -> Result<()> {
    let confirm_map_err = |_| anyhow!("Error with questionnaire, try again later");
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
        .with_default(false)
        .prompt()
        .map_err(confirm_map_err)?;
    if ans {
        raw_config.push_str("save: true\n");
    }

    let ans = Confirm::new("Whether to highlight reply message?")
        .with_default(true)
        .prompt()
        .map_err(confirm_map_err)?;
    if ans {
        raw_config.push_str("highlight: true\n");
    }

    std::fs::write(config_path, raw_config)
        .map_err(|err| anyhow!("Failed to write to config file, {err}"))?;
    Ok(())
}
