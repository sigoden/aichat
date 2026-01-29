use crate::{
    config::{Agent, Config, GlobalConfig},
    utils::*,
};

use anyhow::{anyhow, bail, Context, Result};
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::{HashMap, HashSet},
    fs,
    path::{Path, PathBuf},
};

#[cfg(windows)]
const PATH_SEP: &str = ";";
#[cfg(not(windows))]
const PATH_SEP: &str = ":";

pub fn eval_tool_calls(config: &GlobalConfig, mut calls: Vec<ToolCall>) -> Result<Vec<ToolResult>> {
    let mut output = vec![];
    if calls.is_empty() {
        return Ok(output);
    }
    calls = ToolCall::dedup(calls);
    if calls.is_empty() {
        bail!("The request was aborted because an infinite loop of function calls was detected.")
    }
    let mut is_all_null = true;
    for call in calls {
        // Check if this tool call is in the prior calls buffer, record or respond accordingly.
        if let Some(checker) = &config.read().tool_call_tracker {
            if let Some(msg) = checker.check_loop(&call.clone()) {
                let dup_msg = format!("{{\"tool_call_loop_alert\":{}}}", &msg.trim());
                println!("{}", warning_text(format!("{}: ⚠️ Tool-call loop detected! ⚠️", &call.name).as_str()));
                let val = json!(dup_msg);
                output.push(ToolResult::new(call, val));
                is_all_null = false;
                continue;
            }
            // Config is locked here so we can't record the calls quite yet
        }
        let mut result = call.eval(config)?;
        if result.is_null() {
            result = json!("DONE");
        } else {
            is_all_null = false;
        }
        output.push(ToolResult::new(call, result));
    }
    if is_all_null {
        output = vec![];
    }
    Ok(output)
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ToolResult {
    pub call: ToolCall,
    pub output: Value,
}

impl ToolResult {
    pub fn new(call: ToolCall, output: Value) -> Self {
        Self { call, output }
    }
}

#[derive(Debug, Clone, Default)]
pub struct Functions {
    declarations: Vec<FunctionDeclaration>,
}

impl Functions {
    pub fn init(declarations_path: &Path) -> Result<Self> {
        let declarations: Vec<FunctionDeclaration> = if declarations_path.exists() {
            let ctx = || {
                format!(
                    "Failed to load functions at {}",
                    declarations_path.display()
                )
            };
            let content = fs::read_to_string(declarations_path).with_context(ctx)?;
            serde_json::from_str(&content).with_context(ctx)?
        } else {
            vec![]
        };

        Ok(Self { declarations })
    }

    pub fn find(&self, name: &str) -> Option<&FunctionDeclaration> {
        self.declarations.iter().find(|v| v.name == name)
    }

    pub fn contains(&self, name: &str) -> bool {
        self.declarations.iter().any(|v| v.name == name)
    }

    pub fn declarations(&self) -> &[FunctionDeclaration] {
        &self.declarations
    }

    pub fn is_empty(&self) -> bool {
        self.declarations.is_empty()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionDeclaration {
    pub name: String,
    pub description: String,
    pub parameters: JsonSchema,
    #[serde(skip_serializing, default)]
    pub agent: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonSchema {
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    pub type_value: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub properties: Option<IndexMap<String, JsonSchema>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub items: Option<Box<JsonSchema>>,
    #[serde(rename = "anyOf", skip_serializing_if = "Option::is_none")]
    pub any_of: Option<Vec<JsonSchema>>,
    #[serde(rename = "enum", skip_serializing_if = "Option::is_none")]
    pub enum_value: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub required: Option<Vec<String>>,
}

impl JsonSchema {
    pub fn is_empty_properties(&self) -> bool {
        match &self.properties {
            Some(v) => v.is_empty(),
            None => true,
        }
    }
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct ToolCall {
    pub name: String,
    pub arguments: Value,
    pub id: Option<String>,
}

type CallConfig = (String, String, Vec<String>, HashMap<String, String>);

impl ToolCall {
    pub fn dedup(calls: Vec<Self>) -> Vec<Self> {
        let mut new_calls = vec![];
        let mut seen_ids = HashSet::new();

        for call in calls.into_iter().rev() {
            if let Some(id) = &call.id {
                if !seen_ids.contains(id) {
                    seen_ids.insert(id.clone());
                    new_calls.push(call);
                }
            } else {
                new_calls.push(call);
            }
        }

        new_calls.reverse();
        new_calls
    }

    pub fn new(name: String, arguments: Value, id: Option<String>) -> Self {
        Self {
            name,
            arguments,
            id,
        }
    }

    pub fn eval(&self, config: &GlobalConfig) -> Result<Value> {
        let (call_name, cmd_name, mut cmd_args, envs) = match &config.read().agent {
            Some(agent) => self.extract_call_config_from_agent(config, agent)?,
            None => self.extract_call_config_from_config(config)?,
        };

        let json_data = if self.arguments.is_object() {
            self.arguments.clone()
        } else if let Some(arguments) = self.arguments.as_str() {
            let arguments: Value = serde_json::from_str(arguments).map_err(|_| {
                anyhow!("The call '{call_name}' has invalid arguments: {arguments}")
            })?;
            arguments
        } else {
            bail!(
                "The call '{call_name}' has invalid arguments: {}",
                self.arguments
            );
        };

        cmd_args.push(json_data.to_string());

        let output = match run_llm_function(cmd_name, cmd_args, envs) {
            Ok(Some(contents)) => serde_json::from_str(&contents)
                .ok()
                .unwrap_or_else(|| json!({"output": contents})),
            Ok(None) => Value::Null,
            Err(e) => serde_json::from_str(&e.to_string())
                .ok()
                .unwrap_or_else(|| json!({"output": e.to_string()}))
        };

        Ok(output)
    }

    fn extract_call_config_from_agent(
        &self,
        config: &GlobalConfig,
        agent: &Agent,
    ) -> Result<CallConfig> {
        let function_name = self.name.clone();
        match agent.functions().find(&function_name) {
            Some(function) => {
                let agent_name = agent.name().to_string();
                if function.agent {
                    Ok((
                        format!("{agent_name}-{function_name}"),
                        agent_name,
                        vec![function_name],
                        agent.variable_envs(),
                    ))
                } else {
                    Ok((
                        function_name.clone(),
                        function_name,
                        vec![],
                        Default::default(),
                    ))
                }
            }
            None => self.extract_call_config_from_config(config),
        }
    }

    fn extract_call_config_from_config(&self, config: &GlobalConfig) -> Result<CallConfig> {
        let function_name = self.name.clone();
        match config.read().functions.contains(&function_name) {
            true => Ok((
                function_name.clone(),
                function_name,
                vec![],
                Default::default(),
            )),
            false => bail!("Unexpected call: {function_name} {}", self.arguments),
        }
    }
}

pub fn run_llm_function(
    cmd_name: String,
    cmd_args: Vec<String>,
    mut envs: HashMap<String, String>,
) -> Result<Option<String>> {
    let prompt = format!("Call {cmd_name} {}", cmd_args.join(" "));

    let mut bin_dirs: Vec<PathBuf> = vec![];
    if cmd_args.len() > 1 {
        let dir = Config::agent_functions_dir(&cmd_name).join("bin");
        if dir.exists() {
            bin_dirs.push(dir);
        }
    }
    bin_dirs.push(Config::functions_bin_dir());
    let current_path = std::env::var("PATH").context("No PATH environment variable")?;
    let prepend_path = bin_dirs
        .iter()
        .map(|v| format!("{}{PATH_SEP}", v.display()))
        .collect::<Vec<_>>()
        .join("");
    envs.insert("PATH".into(), format!("{prepend_path}{current_path}"));

    let temp_file = temp_file("-eval-", "");
    envs.insert("LLM_OUTPUT".into(), temp_file.display().to_string());

    #[cfg(windows)]
    let cmd_name = polyfill_cmd_name(&cmd_name, &bin_dirs);
    if *IS_STDOUT_TERMINAL {
        println!("{}", dimmed_text(&prompt));
    }
    let exit_code = run_command(&cmd_name, &cmd_args, Some(envs))
        .map_err(|err| anyhow!("Unable to run {cmd_name}, {err}"))?;
    if exit_code != 0 {
        let tool_error_message = format!("⚠️ Tool call '{cmd_name}' threw exit code {exit_code} ⚠️");
        println!("{}", warning_text(&tool_error_message));
        let tool_error_json = format!("{{\"tool_call_error\":\"{}\"}}", &tool_error_message);
        return Ok(Some(tool_error_json));
    }
    let mut output = None;
    if temp_file.exists() {
        let contents =
            fs::read_to_string(temp_file).context("Failed to retrieve tool call output")?;
        if !contents.is_empty() {
            output = Some(contents);
        }
    };
    Ok(output)
}

#[cfg(windows)]
fn polyfill_cmd_name<T: AsRef<Path>>(cmd_name: &str, bin_dir: &[T]) -> String {
    let cmd_name = cmd_name.to_string();
    if let Ok(exts) = std::env::var("PATHEXT") {
        for name in exts.split(';').map(|ext| format!("{cmd_name}{ext}")) {
            for dir in bin_dir {
                let path = dir.as_ref().join(&name);
                if path.exists() {
                    return name.to_string();
                }
            }
        }
    }
    cmd_name
}

use std::collections::VecDeque;

#[derive(Debug, Clone)]
pub struct ToolCallTracker {
    last_calls: VecDeque<ToolCall>,
    max_repeats: usize,
    chain_len: usize,
}

impl ToolCallTracker {
    pub fn new(max_repeats: usize, chain_len: usize) -> Self {
        Self {
            last_calls: VecDeque::new(),
            max_repeats,
            chain_len,
        }
    }

    pub fn default() -> Self {
        Self::new(2, 3)
    }

    pub fn check_loop(&self, new_call: &ToolCall) -> Option<String> {
        if self.last_calls.len() < self.max_repeats {
            return None;
        }

        // Check if new call matches last call
        if let Some(last) = self.last_calls.back() {
            if self.calls_match(last, new_call) {
                let mut repeat_count = 1;
                for i in (1..self.last_calls.len()).rev() {
                    if self.calls_match(&self.last_calls[i-1], &self.last_calls[i]) {
                        repeat_count += 1;
                        if repeat_count >= self.max_repeats {
                            return Some(self.create_loop_message());
                        }
                    } else {
                        break;
                    }
                }
            }
        }

        // Check for repeating chain
        let start = self.last_calls.len().saturating_sub(self.chain_len);
        let chain: Vec<_> = self.last_calls.iter().skip(start).collect();
        if chain.len() == self.chain_len {
            let mut is_repeating = true;
            for i in 0..chain.len() - 1 {
                if !self.calls_match(chain[i], chain[i + 1]) {
                    is_repeating = false;
                    break;
                }
            }
            if is_repeating && self.calls_match(chain[chain.len() - 1], new_call) {
                return Some(self.create_loop_message());
            }
        }

        None
    }

    fn calls_match(&self, a: &ToolCall, b: &ToolCall) -> bool {
        a.name == b.name && a.arguments == b.arguments
    }

    fn create_loop_message(&self) -> String {
        let message = r#"{"error":{"message":"⚠️ Tool-call loop detected! ⚠️","code":400,"param":"Use the output of the last call to this function and parameter-set then move on to the next step of workflow, change tools/parameters called, or request assistance in the conversation sream"}}"#;

        if self.last_calls.len() >= self.chain_len {
            let start = self.last_calls.len().saturating_sub(self.chain_len);
            let chain: Vec<_> = self.last_calls.iter().skip(start).collect();
            let mut loopset = "[".to_string();
            for (_i, c) in chain.iter().enumerate() {
                loopset += format!("{{\"name\":{},\"parameters\":{}}},", c.name, c.arguments).as_str();
            };
            // Adjust history info array
            let _ = loopset.pop();
            loopset.push(']');
            return format!("{},\"call_history\":{}}}}}", &message[..(&message.len() - 2)], loopset);
            // return serde_json::to_string(&history_message).unwrap_or("⚠️ Tool-call loop detected! ⚠️".to_string())
        } else {
            return message.to_string();
            // return serde_json::to_string(&message).unwrap_or("⚠️ Tool-call loop detected! ⚠️".to_string())
        }
    }

    pub fn record_call(&mut self, call: ToolCall) {
        if self.last_calls.len() >= self.chain_len * self.max_repeats {
            self.last_calls.pop_front();
        }
        self.last_calls.push_back(call);
    }
}
