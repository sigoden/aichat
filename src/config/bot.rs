use super::{Config, GlobalConfig, Role};
use crate::{
    client::Model,
    function::{Functions, FUNCTION_ALL_MATCHER},
};

use anyhow::{Context, Result};
use std::{
    fs::{read_dir, read_to_string},
    path::Path,
};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize)]
pub struct Bot {
    name: String,
    config: BotConfig,
    definition: BotDefinition,
    #[serde(skip)]
    functions: Option<Functions>,
    #[serde(skip)]
    model: Model,
    #[serde(skip)]
    role: Role,
}

impl Bot {
    pub fn init(config: &GlobalConfig, name: &str) -> Result<Self> {
        let config_path = Config::bot_config_file(name)?;
        let definition_path = Config::bot_definition_file(name)?;
        let definition = BotDefinition::load(&definition_path)?;
        let functions_path = Config::bot_functions_file(name)?;
        let functions = if functions_path.exists() {
            Some(Functions::init(&functions_path)?)
        } else {
            None
        };
        let bot_config = BotConfig::init(&config_path)?;
        let model = match &bot_config.model_id {
            Some(v) => Model::from_id(v),
            None => config.read().model.clone(),
        };
        let role = Role::new(name, &definition.instructions);
        Ok(Self {
            name: name.to_string(),
            config: bot_config,
            definition,
            functions,
            model,
            role,
        })
    }

    pub fn export(&self) -> Result<String> {
        let mut value = serde_json::json!(self);
        value["functions_dir"] = Config::bot_functions_dir(&self.name)?
            .display()
            .to_string()
            .into();
        value["config_dir"] = Config::bot_config_dir(&self.name)?
            .display()
            .to_string()
            .into();
        let data = serde_yaml::to_string(&value)?;
        Ok(data)
    }

    pub fn set_model(&mut self, model: &Model) {
        self.config.model_id = Some(model.id())
    }

    pub fn set_temperature(&mut self, value: Option<f64>) {
        self.config.temperature = value;
    }

    pub fn set_top_p(&mut self, value: Option<f64>) {
        self.config.top_p = value;
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn functions(&self) -> Option<&Functions> {
        self.functions.as_ref()
    }

    pub fn has_function(&self, name: &str) -> bool {
        match &self.functions {
            Some(functions) => functions.contains(name),
            None => false,
        }
    }

    pub fn function_matcher(&self) -> Option<&str> {
        match self.functions.is_some() {
            true => Some(FUNCTION_ALL_MATCHER),
            false => None,
        }
    }

    pub fn role(&self) -> &Role {
        &self.role
    }

    pub fn model(&self) -> &Model {
        &self.model
    }

    pub fn temperature(&self) -> Option<f64> {
        self.config.temperature
    }

    pub fn top_p(&self) -> Option<f64> {
        self.config.top_p
    }
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct BotConfig {
    #[serde(rename(serialize = "model", deserialize = "model"))]
    pub model_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_p: Option<f64>,
}

impl BotConfig {
    pub fn init(path: &Path) -> Result<Self> {
        if path.exists() {
            let contents = read_to_string(path).with_context(|| {
                format!("Failed to read bot config file at '{}'", path.display())
            })?;
            let config: Self = serde_yaml::from_str(&contents)
                .with_context(|| format!("Failed to load bot config at '{}'", path.display()))?;
            Ok(config)
        } else {
            Ok(BotConfig::default())
        }
    }
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct BotDefinition {
    name: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    version: String,
    instructions: String,
    #[serde(default)]
    conversation_starters: Vec<String>,
}

impl BotDefinition {
    pub fn load(path: &Path) -> Result<Self> {
        let contents = read_to_string(path)
            .with_context(|| format!("Failed to read bot index file at '{}'", path.display()))?;
        let definition: Self = serde_yaml::from_str(&contents)
            .with_context(|| format!("Failed to load bot at '{}'", path.display()))?;
        Ok(definition)
    }
}

pub fn list_bots() -> Vec<String> {
    list_bots_impl().unwrap_or_default()
}

fn list_bots_impl() -> Result<Vec<String>> {
    let base_dir = Config::bots_functions_dir()?;
    let mut output = vec![];
    for entry in read_dir(base_dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            if let Some(name) = path.file_name() {
                output.push(name.to_string_lossy().to_string())
            }
        }
    }
    Ok(output)
}
