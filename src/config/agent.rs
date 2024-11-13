use super::*;

use crate::{client::Model, function::Functions};

use anyhow::{Context, Result};
use inquire::{validator::Validation, Text};
use std::{fs::read_to_string, path::Path};

use serde::{Deserialize, Serialize};

const DEFAULT_AGENT_NAME: &str = "rag";

#[derive(Debug, Clone, Serialize)]
pub struct Agent {
    name: String,
    config: AgentConfig,
    definition: AgentDefinition,
    #[serde(skip)]
    shared_variables: IndexMap<String, String>,
    #[serde(skip)]
    session_variables: Option<IndexMap<String, String>>,
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
        let functions_dir = Config::agent_functions_dir(name);
        let definition_file_path = functions_dir.join("index.yaml");
        if !definition_file_path.exists() {
            bail!("Unknown agent `{name}`");
        }
        let functions_file_path = functions_dir.join("functions.json");
        let rag_path = Config::agent_rag_file(name, DEFAULT_AGENT_NAME);
        let config_path = Config::agent_config_file(name);
        let mut agent_config = if config_path.exists() {
            AgentConfig::load(&config_path)?
        } else {
            AgentConfig::new(&config.read())
        };
        let mut definition = AgentDefinition::load(&definition_file_path)?;
        let functions = if functions_file_path.exists() {
            Functions::init(&functions_file_path)?
        } else {
            Functions::default()
        };
        definition.replace_tools_placeholder(&functions);

        agent_config.load_envs(&definition.name);

        let model = {
            let config = config.read();
            match agent_config.model_id.as_ref() {
                Some(model_id) => Model::retrieve_chat(&config, model_id)?,
                None => config.current_model().clone(),
            }
        };

        let rag = if rag_path.exists() {
            Some(Arc::new(Rag::load(config, DEFAULT_AGENT_NAME, &rag_path)?))
        } else if !definition.documents.is_empty() && !config.read().print_info_only {
            let mut ans = false;
            if *IS_STDOUT_TERMINAL {
                ans = Confirm::new("The agent has the documents, init RAG?")
                    .with_default(true)
                    .prompt()?;
            }
            if ans {
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
                let rag =
                    Rag::init(config, "rag", &rag_path, &document_paths, abort_signal).await?;
                Some(Arc::new(rag))
            } else {
                None
            }
        } else {
            None
        };

        Ok(Self {
            name: name.to_string(),
            config: agent_config,
            definition,
            shared_variables: Default::default(),
            session_variables: None,
            functions,
            rag,
            model,
        })
    }

    pub fn init_agent_variables(
        agent_variables: &[AgentVariable],
        variables: &IndexMap<String, String>,
        no_interaction: bool,
    ) -> Result<IndexMap<String, String>> {
        let mut output = IndexMap::new();
        if agent_variables.is_empty() {
            return Ok(output);
        }
        let mut printed = false;
        let mut unset_variables = vec![];
        for agent_variable in agent_variables {
            let key = agent_variable.name.clone();
            match variables.get(&key) {
                Some(value) => {
                    output.insert(key, value.clone());
                }
                None => {
                    if let Some(value) = agent_variable.default.clone() {
                        output.insert(key, value);
                        continue;
                    }
                    if no_interaction {
                        continue;
                    }
                    if *IS_STDOUT_TERMINAL {
                        if !printed {
                            println!("⚙ Init agent variables...");
                            printed = true;
                        }
                        let value = Text::new(&format!(
                            "{} ({}):",
                            agent_variable.name, agent_variable.description
                        ))
                        .with_validator(|input: &str| {
                            if input.trim().is_empty() {
                                Ok(Validation::Invalid("This field is required".into()))
                            } else {
                                Ok(Validation::Valid)
                            }
                        })
                        .prompt()?;
                        output.insert(key, value);
                    } else {
                        unset_variables.push(agent_variable)
                    }
                }
            }
        }
        if !unset_variables.is_empty() {
            bail!(
                "The following agent variables are required:\n{}",
                unset_variables
                    .iter()
                    .map(|v| format!("  - {}: {}", v.name, v.description))
                    .collect::<Vec<_>>()
                    .join("\n")
            )
        }
        Ok(output)
    }

    pub fn export(&self) -> Result<String> {
        let mut agent = self.clone();
        agent.definition.instructions = self.interpolated_instructions();
        let mut value = serde_json::json!(agent);
        let variables = self.variables();
        if !variables.is_empty() {
            value["variables"] = serde_json::to_value(variables)?;
        }
        value["functions_dir"] = Config::agent_functions_dir(&self.name)
            .display()
            .to_string()
            .into();
        value["data_dir"] = Config::agent_data_dir(&self.name)
            .display()
            .to_string()
            .into();
        value["config_file"] = Config::agent_config_file(&self.name)
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

    pub fn rag(&self) -> Option<Arc<Rag>> {
        self.rag.clone()
    }

    pub fn conversation_staters(&self) -> &[String] {
        &self.definition.conversation_starters
    }

    pub fn interpolated_instructions(&self) -> String {
        self.definition.interpolated_instructions(self.variables())
    }

    pub fn agent_prelude(&self) -> Option<&str> {
        self.config.agent_prelude.as_deref()
    }

    pub fn set_agent_prelude(&mut self, value: Option<String>) {
        self.config.agent_prelude = value;
    }

    pub fn variables(&self) -> &IndexMap<String, String> {
        match &self.session_variables {
            Some(variables) => variables,
            None => &self.shared_variables,
        }
    }

    pub fn config_variables(&self) -> &IndexMap<String, String> {
        &self.config.variables
    }

    pub fn shared_variables(&self) -> &IndexMap<String, String> {
        &self.shared_variables
    }

    pub fn set_shared_variables(&mut self, shared_variables: IndexMap<String, String>) {
        self.shared_variables = shared_variables;
    }

    pub fn set_session_variables(&mut self, session_variables: Option<IndexMap<String, String>>) {
        self.session_variables = session_variables;
    }

    pub fn set_variable(&mut self, key: &str, value: &str) -> Result<()> {
        let variables = match self.session_variables.as_mut() {
            Some(v) => v,
            None => &mut self.shared_variables,
        };
        if !variables.contains_key(key) {
            bail!("Unknown variable: '{key}'")
        }
        variables.insert(key.to_string(), value.to_string());
        Ok(())
    }

    pub fn defined_variables(&self) -> &[AgentVariable] {
        &self.definition.variables
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
    #[serde(default)]
    pub variables: IndexMap<String, String>,
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

    fn load_envs(&mut self, name: &str) {
        let with_prefix = |v: &str| normalize_env_name(&format!("{name}_{v}"));

        if let Some(v) = read_env_value::<String>(&with_prefix("model")) {
            self.model_id = v;
        }
        if let Some(v) = read_env_value::<f64>(&with_prefix("temperature")) {
            self.temperature = v;
        }
        if let Some(v) = read_env_value::<f64>(&with_prefix("top_p")) {
            self.top_p = v;
        }
        if let Some(v) = read_env_value::<String>(&with_prefix("use_tools")) {
            self.use_tools = v;
        }
        if let Some(v) = read_env_value::<String>(&with_prefix("agent_prelude")) {
            self.agent_prelude = v;
        }
        if let Ok(v) = env::var(with_prefix("variables")) {
            if let Ok(v) = serde_json::from_str(&v) {
                self.variables = v;
            }
        }
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

    fn interpolated_instructions(&self, variables: &IndexMap<String, String>) -> String {
        let mut output = self.instructions.clone();
        for (k, v) in variables {
            output = output.replace(&format!("{{{{{k}}}}}"), v)
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
    let agents_file = Config::functions_dir().join("agents.txt");
    let contents = match read_to_string(agents_file) {
        Ok(v) => v,
        Err(_) => return vec![],
    };
    contents
        .split('\n')
        .filter_map(|line| {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                None
            } else {
                Some(line.to_string())
            }
        })
        .collect()
}
