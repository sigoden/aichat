use std::{fs::read_to_string, path::Path};

use anyhow::{anyhow, Result};
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    /// Openai api key
    pub api_key: String,
    /// Set proxy
    pub proxy: Option<String>,
    /// Used only for debugging
    #[serde(default)]
    pub dry_run: bool,
    /// Predefined rules
    #[serde(default)]
    pub roles: Vec<Role>,
}

impl Config {
    pub fn init(path: &Path) -> Result<Config> {
        let content = read_to_string(path)
            .map_err(|err| anyhow!("Failed to load config at {}, {err}", path.display()))?;
        let config: Config =
            toml::from_str(&content).map_err(|err| anyhow!("Invalid config, {err}"))?;
        Ok(config)
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct Role {
    /// Role name
    pub name: String,
    /// Prompt text send to ai for setting up a rule
    pub prompt: String,
    /// First sentense will append to prompt
    pub first_sentense: String,
}

impl Role {
    pub fn generate(&self, text: &str) -> String {
        format!("{} {} {}", self.prompt, self.first_sentense, text)
    }
}
