use super::*;

use crate::{
    client::{LLMClient, Model}, // Added LLMClient
    function::{run_llm_function, Functions, ToolCall}, // Added ToolCall
};

use anyhow::{Context, Result, bail}; // Added bail here for run_react_loop
use regex::Regex; // Added Regex
use serde_json::Value; // Added Value
use log::info; // Added info
use inquire::{validator::Validation, Text};
use std::{fs::read_to_string, path::Path};

use serde::{Deserialize, Serialize};

const DEFAULT_AGENT_NAME: &str = "rag";

pub type AgentVariables = IndexMap<String, String>;

#[derive(Debug, Clone)]
pub struct Agent {
    name: String,
    config: AgentConfig,
    definition: AgentDefinition,
    shared_variables: AgentVariables,
    session_variables: Option<AgentVariables>,
    shared_dynamic_instructions: Option<String>,
    session_dynamic_instructions: Option<String>,
    functions: Functions,
    rag: Option<Arc<Rag>>,
    model: Model,
    react_scratchpad: Vec<String>,
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
                Some(model_id) => Model::retrieve_model(&config, model_id, ModelType::Chat)?,
                None => config.current_model().clone(),
            }
        };

        let rag = if rag_path.exists() {
            Some(Arc::new(Rag::load(config, DEFAULT_AGENT_NAME, &rag_path)?))
        } else if !definition.documents.is_empty() && !config.read().info_flag {
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
            shared_dynamic_instructions: None,
            session_dynamic_instructions: None,
            functions,
            rag,
            model,
            react_scratchpad: Vec::new(),
        })
    }

    pub fn clear_react_scratchpad(&mut self) {
        self.react_scratchpad.clear();
    }

    pub fn add_to_react_scratchpad(&mut self, entry: String) {
        self.react_scratchpad.push(entry);
    }

    pub fn get_react_scratchpad_content(&self) -> String {
        self.react_scratchpad.join("\n")
    }

    pub fn init_agent_variables(
        agent_variables: &[AgentVariable],
        variables: &AgentVariables,
        no_interaction: bool,
    ) -> Result<AgentVariables> {
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
        let mut value = json!({});
        value["name"] = json!(self.name());
        let variables = self.variables();
        if !variables.is_empty() {
            value["variables"] = serde_json::to_value(variables)?;
        }
        value["config"] = json!(self.config);
        let mut definition = self.definition.clone();
        definition.instructions = self.interpolated_instructions();
        value["definition"] = json!(definition);
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
        let mut output = self
            .session_dynamic_instructions
            .clone()
            .or_else(|| self.shared_dynamic_instructions.clone())
            .or_else(|| self.config.instructions.clone())
            .unwrap_or_else(|| self.definition.instructions.clone());
        for (k, v) in self.variables() {
            output = output.replace(&format!("{{{{{k}}}}}"), v)
        }
        interpolate_variables(&mut output);
        output
    }

    pub fn agent_prelude(&self) -> Option<&str> {
        self.config.agent_prelude.as_deref()
    }

    pub fn variables(&self) -> &AgentVariables {
        match &self.session_variables {
            Some(variables) => variables,
            None => &self.shared_variables,
        }
    }

    pub fn variable_envs(&self) -> HashMap<String, String> {
        self.variables()
            .iter()
            .map(|(k, v)| {
                (
                    format!("LLM_AGENT_VAR_{}", normalize_env_name(k)),
                    v.clone(),
                )
            })
            .collect()
    }

    pub fn config_variables(&self) -> &AgentVariables {
        &self.config.variables
    }

    pub fn shared_variables(&self) -> &AgentVariables {
        &self.shared_variables
    }

    pub fn set_shared_variables(&mut self, shared_variables: AgentVariables) {
        self.shared_variables = shared_variables;
    }

    pub fn set_session_variables(&mut self, session_variables: AgentVariables) {
        self.session_variables = Some(session_variables);
    }

    pub fn defined_variables(&self) -> &[AgentVariable] {
        &self.definition.variables
    }

    pub fn exit_session(&mut self) {
        self.session_variables = None;
        self.session_dynamic_instructions = None;
        self.clear_react_scratchpad(); // Clear scratchpad when session exits
    }

    pub fn is_dynamic_instructions(&self) -> bool {
        self.definition.dynamic_instructions
    }

    pub fn update_shared_dynamic_instructions(&mut self, force: bool) -> Result<()> {
        if self.is_dynamic_instructions() && (force || self.shared_dynamic_instructions.is_none()) {
            self.shared_dynamic_instructions = Some(self.run_instructions_fn()?);
        }
        Ok(())
    }

    pub fn update_session_dynamic_instructions(&mut self, value: Option<String>) -> Result<()> {
        if self.is_dynamic_instructions() {
            let value = match value {
                Some(v) => v,
                None => self.run_instructions_fn()?,
            };
            self.session_dynamic_instructions = Some(value);
        }
        Ok(())
    }

    fn run_instructions_fn(&self) -> Result<String> {
        let value = run_llm_function(
            self.name().to_string(),
            vec!["_instructions".into(), "{}".into()],
            self.variable_envs(),
        )?;
        match value {
            Some(v) => Ok(v),
            _ => bail!("No return value from '_instructions' function"),
        }
    }

    pub async fn run_react_loop(
        &mut self,
        current_user_query: String,
        initial_goal: String, // The overall goal for {{user_goal}}
        client: &LLMClient,
        global_config: &GlobalConfig, // Renamed from 'config' to avoid conflict with self.config
        print_thought: bool,
    ) -> Result<String> {
        self.clear_react_scratchpad();

        const MAX_REACT_ITERATIONS: usize = 10;

        // Regex for parsing LLM output
        // Order: Final Answer, Action, Thought
        // Action can be tool_name({"json_args"}) or None
        let react_parser_re = Regex::new(
            r"(?s)(?:Final Answer:\s*(?P<final_answer>.+)|Action:\s*(?P<action_tool>[a-zA-Z_][a-zA-Z0-9_]*)\s*(?P<action_args>\{.*\})|Action:\s*(?P<action_none>None)|Thought:\s*(?P<thought>.+))"
        ).unwrap();

        // Get the base ReAct instructions template
        let mut react_instructions_template = self.interpolated_instructions();
        if !react_instructions_template.contains("{{user_goal}}") ||
           !react_instructions_template.contains("{{tools_list_for_prompt}}") ||
           !react_instructions_template.contains("{{react_scratchpad_content}}") ||
           !react_instructions_template.contains("{{current_user_query}}") {
            bail!("Agent's instructions are not a valid ReAct meta-prompt. Placeholders missing.");
        }

        // Prepare tools list for prompt
        let tools_list_for_prompt = self
            .functions()
            .declarations()
            .iter()
            .map(|f| format!("- {}: {}", f.name, f.description.lines().next().unwrap_or_default()))
            .collect::<Vec<String>>()
            .join("\n");

        react_instructions_template = react_instructions_template.replace("{{user_goal}}", &initial_goal);
        react_instructions_template = react_instructions_template.replace("{{tools_list_for_prompt}}", &tools_list_for_prompt);

        for i in 0..MAX_REACT_ITERATIONS {
            let scratchpad_content = self.get_react_scratchpad_content();
            let full_prompt = react_instructions_template
                .replace("{{react_scratchpad_content}}", &scratchpad_content)
                .replace("{{current_user_query}}", &current_user_query); // current_user_query might be updated in more advanced scenarios

            // Call LLM
            // Assuming LLMClient has a method like this or adapting send_message_inner
            // For now, let's assume a conceptual direct call.
            // This part needs careful implementation based on LLMClient capabilities.
            // We'll use send_message_inner for now.
            let input_message = Message::new_user(&full_prompt);
            let response_message = client.send_message_inner(
                global_config, // Pass GlobalConfig
                self.model().client(), // Get the GptClient for the agent's model
                &input_message,
                self.temperature(),
                self.top_p(),
                None, // No stream handler for ReAct's synchronous steps
            ).await?;

            let llm_response_text = response_message.content.trim();

            if let Some(captures) = react_parser_re.captures(llm_response_text) {
                if let Some(final_answer_match) = captures.name("final_answer") {
                    let final_answer_content = final_answer_match.as_str().trim().to_string();
                    self.add_to_react_scratchpad(format!("Final Answer: {}", final_answer_content));
                    return Ok(final_answer_content);
                } else if let Some(action_tool_match) = captures.name("action_tool") {
                    let tool_name_str = action_tool_match.as_str().trim().to_string();
                    let args_str = captures.name("action_args")
                        .map_or("{}".to_string(), |m| m.as_str().trim().to_string());
                    
                    self.add_to_react_scratchpad(format!("Action: {} {}", tool_name_str, args_str));

                    match serde_json::from_str::<Value>(&args_str) {
                        Ok(parsed_json_args) => {
                            let tool_call = ToolCall {
                                name: tool_name_str,
                                arguments: parsed_json_args,
                                id: Some(format!("react-{}", i)),
                            };
                            match tool_call.eval(global_config) {
                                Ok(tool_result_value) => {
                                    let observation = format!("Observation: {}", tool_result_value.to_string());
                                    self.add_to_react_scratchpad(observation);
                                }
                                Err(e) => {
                                    let error_observation = format!("Error executing tool {}: {}", tool_call.name, e);
                                    self.add_to_react_scratchpad(format!("Observation: {}", error_observation));
                                }
                            }
                        }
                        Err(e) => {
                            let error_observation = format!("System: Failed to parse JSON arguments for tool {}: {}. Error: {}", tool_name_str, args_str, e);
                            self.add_to_react_scratchpad(error_observation);
                        }
                    }
                } else if captures.name("action_none").is_some() {
                    self.add_to_react_scratchpad("Action: None".to_string());
                    // Loop continues, LLM should provide a new thought or eventually a final answer
                } else if let Some(thought_match) = captures.name("thought") {
                    let thought_content = thought_match.as_str().trim().to_string();
                    self.add_to_react_scratchpad(format!("Thought: {}", thought_content));
                    if print_thought {
                        info!("Thought: {}", thought_content);
                    }
                     // After a thought, we must re-prompt the LLM immediately for an Action or Final Answer.
                    // The current loop structure will do this.
                    // However, ensure the LLM output format for Thought is just "Thought: ..." and doesn't include Action/Final Answer in the same response.
                    // If it does, the regex should capture the Action/Final Answer part first.
                    // The current regex prioritizes Final Answer, then Action, then Thought.
                    // If only a thought is produced, the loop continues and the scratchpad grows.
                } else {
                    // This case should ideally not be reached if regex is comprehensive
                    let system_message = "System: Invalid response format. Could not parse Thought, Action, or Final Answer.".to_string();
                    self.add_to_react_scratchpad(system_message);
                }
            } else {
                // LLM response did not match any ReAct directives
                let system_message = format!("System: Invalid response format. The response was '{}'. Please use Thought/Action/Final Answer.", llm_response_text);
                self.add_to_react_scratchpad(system_message);
            }
        }

        bail!("Agent exceeded max ReAct iterations ({}).", MAX_REACT_ITERATIONS)
    }

    pub async fn run_react_loop(
        &mut self,
        current_user_query: String,
        initial_goal: String, // The overall goal for {{user_goal}}
        client: &LLMClient,
        global_config: &GlobalConfig, 
        print_thought: bool,
    ) -> Result<String> {
        self.clear_react_scratchpad();

        const MAX_REACT_ITERATIONS: usize = 10;

        // Regex for parsing LLM output. Prioritizes Final Answer, then Action, then Thought.
        // Action: tool_name({"json_args"}) OR Action: None
        let react_parser_re = Regex::new(
            r"(?s)^\s*(?:Final Answer:\s*(?P<final_answer>.+)|Action:\s*(?P<action_tool>[a-zA-Z_][a-zA-Z0-9_]*)\s*(?P<action_args>\{.*?\})|Action:\s*(?P<action_none>None)|Thought:\s*(?P<thought>.+))"
        ).context("Failed to compile ReAct parser regex")?;

        // Get the base ReAct instructions template
        // This template is expected to be set as the agent's instructions.
        let mut react_instructions_template = self.interpolated_instructions(); 

        // Validate that the template contains necessary placeholders
        if !react_instructions_template.contains("{{user_goal}}") ||
           !react_instructions_template.contains("{{tools_list_for_prompt}}") ||
           !react_instructions_template.contains("{{react_scratchpad_content}}") ||
           !react_instructions_template.contains("{{current_user_query}}") {
            bail!("Agent's instructions do not conform to the expected ReAct meta-prompt format. Key placeholders like '{{user_goal}}', '{{tools_list_for_prompt}}', '{{react_scratchpad_content}}', or '{{current_user_query}}' are missing.");
        }

        // Prepare tools list for prompt
        let tools_list_for_prompt = self
            .functions()
            .declarations()
            .iter()
            .map(|f| {
                let desc = f.description.lines().next().unwrap_or("No description.");
                format!("- {}: {}", f.name, desc)
            })
            .collect::<Vec<String>>()
            .join("\n");
        
        // Pre-fill parts of the template that don't change per iteration
        react_instructions_template = react_instructions_template.replace("{{user_goal}}", &initial_goal);
        react_instructions_template = react_instructions_template.replace("{{tools_list_for_prompt}}", &tools_list_for_prompt);

        for i in 0..MAX_REACT_ITERATIONS {
            let scratchpad_content = self.get_react_scratchpad_content();
            
            // Construct the full prompt for this iteration
            let full_prompt = react_instructions_template
                .replace("{{react_scratchpad_content}}", &scratchpad_content)
                .replace("{{current_user_query}}", &current_user_query); // current_user_query could be updated based on thoughts in more advanced setups

            let input_message = Message::new_user(&full_prompt);
            
            let response_message = client.send_message_inner(
                global_config,
                self.model().client(), 
                &input_message,
                self.temperature(),
                self.top_p(),
                None, // No streaming for ReAct individual steps
            ).await.context("LLM call failed in ReAct loop")?;

            let llm_response_text = response_message.content.trim();
            self.add_to_react_scratchpad(llm_response_text.to_string()); // Add raw LLM response to scratchpad for full history

            if let Some(captures) = react_parser_re.captures(llm_response_text) {
                if let Some(final_answer_match) = captures.name("final_answer") {
                    let final_answer_content = final_answer_match.as_str().trim().to_string();
                    // No need to add "Final Answer: ..." to scratchpad again as raw response is already added.
                    return Ok(final_answer_content);
                } else if let Some(action_tool_match) = captures.name("action_tool") {
                    let tool_name_str = action_tool_match.as_str().trim().to_string();
                    // Ensure action_args capture group exists before trying to use it.
                    let args_str = captures.name("action_args").map_or("{}".to_string(), |m| m.as_str().trim().to_string());
                    
                    // Raw action already in scratchpad. Now process it.
                    match serde_json::from_str::<Value>(&args_str) {
                        Ok(parsed_json_args) => {
                            let tool_call = ToolCall {
                                name: tool_name_str.clone(),
                                arguments: parsed_json_args,
                                id: Some(format!("react-{}", i)),
                            };
                            match tool_call.eval(global_config) {
                                Ok(tool_result_value) => {
                                    let observation = format!("Observation: {}", tool_result_value.to_string());
                                    self.add_to_react_scratchpad(observation);
                                }
                                Err(e) => {
                                    let error_observation = format!("Observation: Error executing tool '{}': {}", tool_name_str, e);
                                    self.add_to_react_scratchpad(error_observation);
                                }
                            }
                        }
                        Err(e) => {
                            let error_observation = format!("Observation: System failed to parse JSON arguments for tool '{}' (args: '{}'). Error: {}", tool_name_str, args_str, e);
                            self.add_to_react_scratchpad(error_observation);
                        }
                    }
                } else if captures.name("action_none").is_some() {
                    // "Action: None" is already in scratchpad from raw response. Loop continues.
                } else if let Some(thought_match) = captures.name("thought") {
                    let thought_content = thought_match.as_str().trim().to_string();
                    // "Thought: ..." is already in scratchpad.
                    if print_thought {
                        info!("Thought: {}", thought_content);
                    }
                    // If only a thought is produced, the loop continues.
                    // The LLM should then produce an Action or Final Answer in the next step.
                } else {
                    // This should not be reached if regex is correct and LLM adheres to one of Thought/Action/Final Answer.
                    // Adding a generic observation if the LLM output was captured by regex but not by a specific group.
                    let observation = format!("Observation: System received an LLM response that was partially matched but not identifiable as Thought, Action, or Final Answer: '{}'", llm_response_text);
                    self.add_to_react_scratchpad(observation);
                }
            } else {
                // LLM response did not match any ReAct directives.
                let observation = format!("Observation: System received an invalid response format. The response was '{}'. Please use Thought/Action/Final Answer.", llm_response_text);
                self.add_to_react_scratchpad(observation);
            }
        }

        bail!("Agent exceeded max ReAct iterations ({}) without reaching a Final Answer.", MAX_REACT_ITERATIONS)
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_p: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub use_tools: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent_prelude: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub instructions: Option<String>,
    #[serde(default, skip_serializing_if = "IndexMap::is_empty")]
    pub variables: AgentVariables,
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
        if let Some(v) = read_env_value::<String>(&with_prefix("instructions")) {
            self.instructions = v;
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
    #[serde(default)]
    pub instructions: String,
    #[serde(default)]
    pub dynamic_instructions: bool,
    #[serde(default)]
    pub is_react_agent: bool, // Added is_react_agent flag
    #[serde(default)]
    pub variables: Vec<AgentVariable>,
    #[serde(default)]
    pub conversation_starters: Vec<String>,
    #[serde(default)]
    pub documents: Vec<String>,
}

impl AgentDefinition {
    // Expose is_react_agent via a getter
    pub fn is_react_agent(&self) -> bool {
        self.is_react_agent
    }

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

pub fn complete_agent_variables(agent_name: &str) -> Vec<(String, Option<String>)> {
    let index_path = Config::agent_functions_dir(agent_name).join("index.yaml");
    if !index_path.exists() {
        return vec![];
    }
    let Ok(definition) = AgentDefinition::load(&index_path) else {
        return vec![];
    };
    definition
        .variables
        .iter()
        .map(|v| {
            let description = match &v.default {
                Some(default) => format!("{} [default: {default}]", v.description),
                None => v.description.clone(),
            };
            (format!("{}=", v.name), Some(description))
        })
        .collect()
}
