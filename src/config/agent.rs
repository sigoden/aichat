use super::*;

use crate::{client::Model, function::Functions};

use anyhow::{Context, Result};
use inquire::{validator::Validation, Text};
use std::{
    fs::{self, read_to_string},
    path::Path,
};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize)]
pub struct Agent {
    name: String,
    config: AgentConfig,
    definition: AgentDefinition,
    #[serde(skip)]
    functions: Functions,
    #[serde(skip)]
    rag: Option<Arc<Rag>>,
    #[serde(skip)]
    model: Model,
}

impl Agent {
    pub async fn init(
        config: &GlobalConfig,
        name: &str,
        abort_signal: AbortSignal,
    ) -> Result<Self> {
        let functions_dir = Config::agent_functions_dir(name)?;
        let definition_file_path = functions_dir.join("index.yaml");
        let functions_file_path = functions_dir.join("functions.json");
        let variables_path = Config::agent_variables_file(name)?;
        let rag_path = Config::agent_rag_file(name, "rag")?;
        let config_path = Config::agent_config_file(name)?;
        let agent_config = if config_path.exists() {
            AgentConfig::load(&config_path)?
        } else {
            AgentConfig::new(&config.read())
        };
        let mut definition = AgentDefinition::load(&definition_file_path)?;
        init_variables(&variables_path, &mut definition.variables)
            .context("Failed to init variables")?;

        let functions = if functions_file_path.exists() {
            Functions::init(&functions_file_path)?
        } else {
            Functions::default()
        };
        definition.replace_tools_placeholder(&functions);

        let model = {
            let config = config.read();
            match agent_config.model_id.as_ref() {
                Some(model_id) => Model::retrieve_chat(&config, model_id)?,
                None => config.current_model().clone(),
            }
        };

        let rag = if rag_path.exists() {
            Some(Arc::new(Rag::load(config, "rag", &rag_path)?))
        } else if !definition.documents.is_empty() {
            println!("The agent has the documents, initializing RAG...");
            let mut document_paths = vec![];
            for path in &definition.documents {
                if is_url(path) {
                    document_paths.push(path.to_string());
                } else {
                    let new_path = safe_join_path(&functions_dir, path)
                        .ok_or_else(|| anyhow!("Invalid document path: '{path}'"))?;
                    document_paths.push(new_path.display().to_string())
                }
            }
            Some(Arc::new(
                Rag::init(config, "rag", &rag_path, &document_paths, abort_signal).await?,
            ))
        } else {
            None
        };

        Ok(Self {
            name: name.to_string(),
            config: agent_config,
            definition,
            functions,
            rag,
            model,
        })
    }

    pub fn save_config(&self) -> Result<()> {
        let config_path = Config::agent_config_file(&self.name)?;
        ensure_parent_exists(&config_path)?;
        let content = serde_yaml::to_string(&self.config)?;
        fs::write(&config_path, content).with_context(|| {
            format!("Failed to save agent config to '{}'", config_path.display())
        })?;

        println!("✨ Saved agent config to '{}'", config_path.display());
        Ok(())
    }

    pub fn export(&self) -> Result<String> {
        let mut agent = self.clone();
        agent.definition.instructions = self.interpolated_instructions();
        let mut value = serde_json::json!(agent);
        value["functions_dir"] = Config::agent_functions_dir(&self.name)?
            .display()
            .to_string()
            .into();
        value["config_dir"] = Config::agent_config_dir(&self.name)?
            .display()
            .to_string()
            .into();
        value["variables_file"] = Config::agent_variables_file(&self.name)?
            .display()
            .to_string()
            .into();
        let data = serde_yaml::to_string(&value)?;
        Ok(data)
    }

    pub fn banner(&self) -> String {
        self.definition.banner()
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn functions(&self) -> &Functions {
        &self.functions
    }

    pub fn definition(&self) -> &AgentDefinition {
        &self.definition
    }

    pub fn rag(&self) -> Option<Arc<Rag>> {
        self.rag.clone()
    }

    pub fn conversation_staters(&self) -> &[String] {
        &self.definition.conversation_starters
    }

    pub fn interpolated_instructions(&self) -> String {
        self.definition.interpolated_instructions()
    }

    pub fn agent_prelude(&self) -> Option<&str> {
        self.config.agent_prelude.as_deref()
    }

    pub fn set_agent_prelude(&mut self, value: Option<String>) {
        self.config.agent_prelude = value;
    }

    pub fn variables(&self) -> &[AgentVariable] {
        &self.definition.variables
    }

    pub fn set_variable(&mut self, key: &str, value: &str) -> Result<()> {
        match self.definition.variables.iter_mut().find(|v| v.name == key) {
            Some(variable) => {
                variable.value = value.to_string();
                let variables_path = Config::agent_variables_file(&self.name)?;
                save_variables(&variables_path, self.variables())?;
                Ok(())
            }
            None => bail!("Unknown variable '{key}'"),
        }
    }
}

impl RoleLike for Agent {
    fn to_role(&self) -> Role {
        let prompt = self.interpolated_instructions();
        let mut role = Role::new("", &prompt);
        role.sync(self);
        role
    }

    fn model(&self) -> &Model {
        &self.model
    }

    fn model_mut(&mut self) -> &mut Model {
        &mut self.model
    }

    fn temperature(&self) -> Option<f64> {
        self.config.temperature
    }

    fn top_p(&self) -> Option<f64> {
        self.config.top_p
    }

    fn use_tools(&self) -> Option<String> {
        self.config.use_tools.clone()
    }

    fn set_model(&mut self, model: &Model) {
        self.config.model_id = Some(model.id());
        self.model = model.clone();
    }

    fn set_temperature(&mut self, value: Option<f64>) {
        self.config.temperature = value;
    }

    fn set_top_p(&mut self, value: Option<f64>) {
        self.config.top_p = value;
    }

    fn set_use_tools(&mut self, value: Option<String>) {
        self.config.use_tools = value;
    }
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct AgentConfig {
    #[serde(rename(serialize = "model", deserialize = "model"))]
    pub model_id: Option<String>,
    pub temperature: Option<f64>,
    pub top_p: Option<f64>,
    pub use_tools: Option<String>,
    pub agent_prelude: Option<String>,
}

impl AgentConfig {
    pub fn new(config: &Config) -> Self {
        Self {
            use_tools: config.use_tools.clone(),
            agent_prelude: config.agent_prelude.clone(),
            ..Default::default()
        }
    }

    pub fn load(path: &Path) -> Result<Self> {
        let contents = read_to_string(path)
            .with_context(|| format!("Failed to read agent config file at '{}'", path.display()))?;
        let config: Self = serde_yaml::from_str(&contents)
            .with_context(|| format!("Failed to load agent config at '{}'", path.display()))?;
        Ok(config)
    }
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct AgentDefinition {
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub version: String,
    pub instructions: String,
    #[serde(default)]
    pub variables: Vec<AgentVariable>,
    #[serde(default)]
    pub conversation_starters: Vec<String>,
    #[serde(default)]
    pub documents: Vec<String>,
}

impl AgentDefinition {
    pub fn load(path: &Path) -> Result<Self> {
        let contents = read_to_string(path)
            .with_context(|| format!("Failed to read agent index file at '{}'", path.display()))?;
        let definition: Self = serde_yaml::from_str(&contents)
            .with_context(|| format!("Failed to load agent index at '{}'", path.display()))?;
        Ok(definition)
    }

    fn banner(&self) -> String {
        let AgentDefinition {
            name,
            description,
            version,
            conversation_starters,
            ..
        } = self;
        let starters = if conversation_starters.is_empty() {
            String::new()
        } else {
            let starters = conversation_starters
                .iter()
                .map(|v| format!("- {v}"))
                .collect::<Vec<_>>()
                .join("\n");
            format!(
                r#"

## Conversation Starters
{starters}"#
            )
        };
        format!(
            r#"# {name} {version}
{description}{starters}"#
        )
    }

    fn interpolated_instructions(&self) -> String {
        let mut output = self.instructions.clone();
        for variable in &self.variables {
            output = output.replace(&format!("{{{{{}}}}}", variable.name), &variable.value)
        }
        interpolate_variables(&mut output);
        output
    }

    fn replace_tools_placeholder(&mut self, functions: &Functions) {
        let tools_placeholder: &str = "{{__tools__}}";
        if self.instructions.contains(tools_placeholder) {
            let tools = functions
                .declarations()
                .iter()
                .enumerate()
                .map(|(i, v)| {
                    let description = match v.description.split_once('\n') {
                        Some((v, _)) => v,
                        None => &v.description,
                    };
                    format!("{}. {}: {description}", i + 1, v.name)
                })
                .collect::<Vec<String>>()
                .join("\n");
            self.instructions = self.instructions.replace(tools_placeholder, &tools);
        }
    }
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct AgentVariable {
    pub name: String,
    pub description: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default: Option<String>,
    #[serde(skip_deserializing, default)]
    pub value: String,
}

pub fn list_agents() -> Vec<String> {
    list_agents_impl().unwrap_or_default()
}

fn list_agents_impl() -> Result<Vec<String>> {
    let base_dir = Config::functions_dir()?;
    let contents = read_to_string(base_dir.join("agents.txt"))?;
    let agents = contents
        .split('\n')
        .filter_map(|line| {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                None
            } else {
                Some(line.to_string())
            }
        })
        .collect();
    Ok(agents)
}

fn init_variables(variables_path: &Path, variables: &mut [AgentVariable]) -> Result<()> {
    if variables.is_empty() {
        return Ok(());
    }
    let variable_values = if variables_path.exists() {
        let content = read_to_string(variables_path).with_context(|| {
            format!(
                "Failed to read variables from '{}'",
                variables_path.display()
            )
        })?;
        let variable_values: IndexMap<String, String> = serde_yaml::from_str(&content)?;
        variable_values
    } else {
        Default::default()
    };
    let mut initialized = false;
    for variable in variables.iter_mut() {
        match variable_values.get(&variable.name) {
            Some(value) => variable.value = value.to_string(),
            None => {
                if !initialized {
                    println!("The agent has the variables and is initializing them...");
                    initialized = true;
                }
                if *IS_STDOUT_TERMINAL {
                    let mut text =
                        Text::new(&variable.description).with_validator(|input: &str| {
                            if input.trim().is_empty() {
                                Ok(Validation::Invalid("This field is required".into()))
                            } else {
                                Ok(Validation::Valid)
                            }
                        });
                    if let Some(default) = &variable.default {
                        text = text.with_default(default);
                    }
                    let value = text.prompt()?;
                    variable.value = value;
                } else {
                    bail!("Failed to init agent variables in the script mode.");
                }
            }
        }
    }
    if initialized {
        save_variables(variables_path, variables)?;
    }
    Ok(())
}

fn save_variables(variables_path: &Path, variables: &[AgentVariable]) -> Result<()> {
    ensure_parent_exists(variables_path)?;
    let variable_values: IndexMap<String, String> = variables
        .iter()
        .map(|v| (v.name.clone(), v.value.clone()))
        .collect();
    let content = serde_yaml::to_string(&variable_values)?;
    fs::write(variables_path, content)
        .with_context(|| format!("Failed to save variables to '{}'", variables_path.display()))?;
    Ok(())
}
