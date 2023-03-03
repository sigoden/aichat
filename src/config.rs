use std::{fs::read_to_string, path::Path};

use anyhow::{anyhow, Result};
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    /// Openai api key
    pub api_key: String,
    /// What sampling temperature to use, between 0 and 2
    pub temperature: Option<f64>,
    /// Set proxy
    pub proxy: Option<String>,
    /// Used only for debugging
    #[serde(default)]
    pub dry_run: bool,
    /// Predefined roles
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
    /// Prompt text send to ai for setting up a role
    pub prompt: String,
}

impl Role {
    pub fn generate(&self, text: &str) -> String {
        format!("{} {}", self.prompt, text)
    }
}
