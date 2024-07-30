use crate::{
    config::{Config, GlobalConfig},
    utils::*,
};

use anyhow::{anyhow, bail, Context, Result};
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::{HashMap, HashSet},
    fs,
    path::Path,
};

pub type ToolResults = (Vec<ToolResult>, String);

pub fn eval_tool_calls(config: &GlobalConfig, mut calls: Vec<ToolCall>) -> Result<Vec<ToolResult>> {
    let mut output = vec![];
    if calls.is_empty() {
        return Ok(output);
    }
    calls = ToolCall::dedup(calls);
    if calls.is_empty() {
        bail!("The request was aborted because an infinite loop of function calls was detected.")
    }
    for call in calls {
        let result = call.eval(config)?;
        output.push(ToolResult::new(call, result));
    }
    Ok(output)
}

pub fn need_send_tool_results(arr: &[ToolResult]) -> bool {
    arr.iter().any(|v| !v.output.is_null())
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
    #[serde(rename = "type")]
    pub type_value: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub properties: Option<IndexMap<String, JsonSchema>>,
    #[serde(rename = "enum", skip_serializing_if = "Option::is_none")]
    pub enum_value: Option<Vec<String>>,
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
        let function_name = self.name.clone();
        let (call_name, cmd_name, mut cmd_args, mut envs) = match &config.read().agent {
            Some(agent) => match agent.functions().find(&function_name) {
                Some(function) => {
                    if function.agent {
                        let envs: HashMap<String, String> = agent
                            .variables()
                            .iter()
                            .map(|v| {
                                (
                                    format!("LLM_AGENT_VAR_{}", normalize_env_name(&v.name)),
                                    v.value.clone(),
                                )
                            })
                            .collect();
                        (
                            format!("{}:{}", agent.name(), function_name),
                            agent.name().to_string(),
                            vec![function_name],
                            envs,
                        )
                    } else {
                        (
                            function_name.clone(),
                            function_name,
                            vec![],
                            Default::default(),
                        )
                    }
                }
                None => bail!("Unexpected call {function_name} {}", self.arguments),
            },
            None => match config.read().functions.contains(&function_name) {
                true => (
                    function_name.clone(),
                    function_name,
                    vec![],
                    Default::default(),
                ),
                false => bail!("Unexpected call: {function_name} {}", self.arguments),
            },
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
        let prompt = format!("Call {cmd_name} {}", cmd_args.join(" "));

        let bin_dir = Config::functions_bin_dir()?;
        if bin_dir.exists() {
            envs.insert("PATH".into(), prepend_env_path(&bin_dir)?);
        }
        let temp_file = temp_file("-eval-", "");
        envs.insert("LLM_OUTPUT".into(), temp_file.display().to_string());

        #[cfg(windows)]
        let cmd_name = polyfill_cmd_name(&cmd_name, &bin_dir);
        println!("{}", dimmed_text(&prompt));
        let exit_code = run_command(&cmd_name, &cmd_args, Some(envs))
            .map_err(|err| anyhow!("Unable to run {cmd_name}, {err}"))?;
        if exit_code != 0 {
            bail!("Tool call exit with {exit_code}");
        }
        let output = if temp_file.exists() {
            let contents =
                fs::read_to_string(temp_file).context("Failed to retrieve tool call output")?;

            serde_json::from_str(&contents)
                .ok()
                .unwrap_or_else(|| json!({"result": contents}))
        } else {
            Value::Null
        };

        Ok(output)
    }
}

fn prepend_env_path(bin_dir: &Path) -> Result<String> {
    let current_path = std::env::var("PATH").context("No PATH environment variable")?;

    let new_path = if cfg!(target_os = "windows") {
        format!("{};{}", bin_dir.display(), current_path)
    } else {
        format!("{}:{}", bin_dir.display(), current_path)
    };
    Ok(new_path)
}

#[cfg(windows)]
fn polyfill_cmd_name(cmd_name: &str, bin_dir: &std::path::Path) -> String {
    let mut cmd_name = cmd_name.to_string();
    if let Ok(exts) = std::env::var("PATHEXT") {
        if let Some(cmd_path) = exts
            .split(';')
            .map(|ext| bin_dir.join(format!("{}{}", cmd_name, ext)))
            .find(|path| path.exists())
        {
            cmd_name = cmd_path.display().to_string();
        }
    }
    cmd_name
}
