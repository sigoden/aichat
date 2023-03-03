use std::{
    env,
    fs::{self, read_to_string},
    path::{Path, PathBuf},
};

use anyhow::{anyhow, Result};
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
    pub fn init(path: &Path) -> Result<Config> {
        let content = read_to_string(path)
            .map_err(|err| anyhow!("Failed to load config at {}, {err}", path.display()))?;
        let mut config: Config =
            serde_yaml::from_str(&content).map_err(|err| anyhow!("Invalid config, {err}"))?;
        config.load_roles()?;
        Ok(config)
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
            fs::create_dir_all(&path).map_err(|err| {
                anyhow!("Failed to create config dir at {}, {err}", path.display())
            })?;
        }
        path.push(name);
        Ok(path)
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

    pub fn messages_file() -> Result<PathBuf> {
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

impl Role {
    pub fn generate(&self, text: &str) -> String {
        format!("{} {}", self.prompt, text)
    }
}
