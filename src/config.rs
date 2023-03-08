use std::{
    cell::RefCell,
    env,
    fs::{create_dir_all, read_to_string, File, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    process::exit,
    sync::Arc,
};

use anyhow::{anyhow, Context, Result};
use inquire::{Confirm, Text};
use serde::{Deserialize, Serialize};

use crate::utils::{emphasis, now};

const CONFIG_FILE_NAME: &str = "config.yaml";
const ROLES_FILE_NAME: &str = "roles.yaml";
const HISTORY_FILE_NAME: &str = "history.txt";
const MESSAGE_FILE_NAME: &str = "messages.md";
const TEMP_ROLE_NAME: &str = "%TEMP%";
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
pub struct Config {
    /// Openai api key
    pub api_key: String,
    /// What sampling temperature to use, between 0 and 2
    pub temperature: Option<f64>,
    /// Whether to persistently save chat messages
    #[serde(default)]
    pub save: bool,
    /// Whether to disable highlight
    #[serde(default = "highlight_value")]
    pub highlight: bool,
    /// Set proxy
    pub proxy: Option<String>,
    /// Used only for debugging
    #[serde(default)]
    pub dry_run: bool,
    /// Predefined roles
    #[serde(default, skip)]
    pub roles: Vec<Role>,
    /// Current selected role
    #[serde(default, skip)]
    pub role: Option<Role>,
}

pub type SharedConfig = Arc<RefCell<Config>>;

impl Config {
    pub fn init(is_interactive: bool) -> Result<Config> {
        let config_path = Config::config_file()?;
        if is_interactive && !config_path.exists() {
            create_config_file(&config_path)?;
        }
        let content = read_to_string(&config_path)
            .with_context(|| format!("Failed to load config at {}", config_path.display()))?;
        let mut config: Config = serde_yaml::from_str(&content)
            .with_context(|| format!("Invalid config at {}", config_path.display()))?;
        config.load_roles()?;
        Ok(config)
    }

    pub fn find_role(&self, name: &str) -> Option<Role> {
        self.roles.iter().find(|v| v.name == name).cloned()
    }

    pub fn config_dir() -> Result<PathBuf> {
        let env_name = format!(
            "{}_CONFIG_DIR",
            env!("CARGO_CRATE_NAME").to_ascii_uppercase()
        );
        let path = match env::var(env_name) {
            Ok(v) => PathBuf::from(v),
            Err(_) => {
                let mut dir = dirs::config_dir().ok_or_else(|| anyhow!("Not found config dir"))?;
                dir.push(env!("CARGO_CRATE_NAME"));
                dir
            }
        };
        if !path.exists() {
            create_dir_all(&path).map_err(|err| {
                anyhow!("Failed to create config dir at {}, {err}", path.display())
            })?;
        }
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
                if v.name == TEMP_ROLE_NAME {
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

    pub fn roles_file() -> Result<PathBuf> {
        Self::local_file(ROLES_FILE_NAME)
    }

    pub fn history_file() -> Result<PathBuf> {
        Self::local_file(HISTORY_FILE_NAME)
    }

    pub fn messages_file() -> Result<PathBuf> {
        Self::local_file(MESSAGE_FILE_NAME)
    }

    pub fn change_role(&mut self, name: &str) -> String {
        match self.find_role(name) {
            Some(role) => {
                let temperature = match role.temperature {
                    Some(v) => format!("{v}"),
                    None => "null".into(),
                };
                let output = format!(
                    "{}: {}\n{}: {}\n{}: {}",
                    emphasis("name"),
                    role.name,
                    emphasis("prompt"),
                    role.prompt.trim(),
                    emphasis("temperature"),
                    temperature
                );
                self.role = Some(role);
                output
            }
            None => "Error: Unknown role".into(),
        }
    }

    pub fn create_temp_role(&mut self, prompt: &str) {
        self.role = Some(Role {
            name: TEMP_ROLE_NAME.into(),
            prompt: prompt.into(),
            temperature: self.temperature,
        });
    }

    pub fn get_prompt(&self) -> Option<String> {
        self.role.as_ref().and_then(|v| {
            if v.prompt.is_empty() {
                None
            } else {
                Some(v.prompt.to_string())
            }
        })
    }

    pub fn get_temperature(&self) -> Option<f64> {
        self.role
            .as_ref()
            .and_then(|v| v.temperature)
            .or(self.temperature)
    }

    pub fn merge_prompt(&self, content: &str) -> String {
        match self.get_prompt() {
            Some(prompt) => format!("{}\n{content}", prompt.trim()),
            None => content.to_string(),
        }
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
        let role_name = self
            .role
            .as_ref()
            .map(|v| v.name.to_string())
            .unwrap_or("-".into());
        let items = vec![
            ("config_file", file_info(&Config::config_file()?)),
            ("roles_file", file_info(&Config::roles_file()?)),
            ("messages_file", file_info(&Config::messages_file()?)),
            ("role", role_name),
            ("api_key", self.api_key.clone()),
            ("temperature", temperature),
            ("save", self.save.to_string()),
            ("highlight", self.highlight.to_string()),
            ("proxy", proxy),
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

    pub fn update(&mut self, data: &str) -> Result<String> {
        let parts: Vec<&str> = data.split_whitespace().collect();
        if parts.len() != 2 {
            return Ok("Usage: .set <key> <value>. If value is null, unset key.".into());
        }
        let key = parts[0];
        let value = parts[1];
        let unset = value == "null";
        match key {
            "api_key" => {
                if unset {
                    return Ok("Error: Not allowed".into());
                } else {
                    self.api_key = value.to_string();
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
            _ => return Ok(format!("Error: Unknown key `{key}`")),
        }
        Ok("".into())
    }

    fn open_message_file(&self) -> Result<File> {
        let path = Config::messages_file()?;
        OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .with_context(|| format!("Failed to create/append {}", path.display()))
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
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Role {
    /// Role name
    pub name: String,
    /// Prompt text send to ai for setting up a role
    pub prompt: String,
    /// What sampling temperature to use, between 0 and 2
    pub temperature: Option<f64>,
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

    std::fs::write(config_path, raw_config).with_context(|| "Failed to write to config file")?;
    Ok(())
}

fn highlight_value() -> bool {
    true
}
