use crate::{
    config::{Config, GlobalConfig},
    utils::*,
};

use anyhow::{anyhow, bail, Context, Result};
use fancy_regex::Regex;
use indexmap::{IndexMap, IndexSet};
use inquire::{validator::Validation, Text};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::{HashMap, HashSet},
    fs,
    path::Path,
};

pub const SELECTED_ALL_FUNCTIONS: &str = ".*";
pub type ToolResults = (Vec<ToolResult>, String);
pub type FunctionsFilter = String;

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
    names: IndexSet<String>,
    declarations: Vec<FunctionDeclaration>,
}

impl Functions {
    pub fn init(declarations_path: &Path) -> Result<Self> {
        let declarations: Vec<FunctionDeclaration> = if declarations_path.exists() {
            let ctx = || {
                format!(
                    "Failed to load function declarations at {}",
                    declarations_path.display()
                )
            };
            let content = fs::read_to_string(declarations_path).with_context(ctx)?;
            serde_json::from_str(&content).with_context(ctx)?
        } else {
            vec![]
        };

        let names = declarations.iter().map(|v| v.name.clone()).collect();

        Ok(Self {
            names,
            declarations,
        })
    }

    pub fn select(&self, filter: &FunctionsFilter) -> Option<Vec<FunctionDeclaration>> {
        let regex = Regex::new(&format!("^({filter})$")).ok()?;
        let output: Vec<FunctionDeclaration> = self
            .declarations
            .iter()
            .filter(|v| regex.is_match(&v.name).unwrap_or_default())
            .cloned()
            .collect();
        if output.is_empty() {
            None
        } else {
            Some(output)
        }
    }

    pub fn contains(&self, name: &str) -> bool {
        self.names.contains(name)
    }

    pub fn is_empty(&self) -> bool {
        self.names.is_empty()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionDeclaration {
    pub name: String,
    pub description: String,
    pub parameters: JsonSchema,
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
        let is_dangerously = config.read().is_dangerously_function(&function_name);
        let (call_name, cmd_name, mut cmd_args) = match &config.read().agent {
            Some(agent) => {
                if !agent.functions().contains(&function_name) {
                    bail!(
                        "Unexpected call: {} {function_name} {}",
                        agent.name(),
                        self.arguments
                    );
                }
                (
                    format!("{}:{}", agent.name(), function_name),
                    agent.name().to_string(),
                    vec![function_name],
                )
            }
            None => {
                if !config.read().functions.contains(&function_name) {
                    bail!("Unexpected call: {function_name} {}", self.arguments);
                }
                (function_name.clone(), function_name, vec![])
            }
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

        let mut envs = HashMap::new();
        let bin_dir = Config::functions_bin_dir()?;
        if bin_dir.exists() {
            envs.insert("PATH".into(), prepend_env_path(&bin_dir)?);
        }

        #[cfg(windows)]
        let cmd_name = polyfill_cmd_name(&cmd_name, &bin_dir);

        let output = if is_dangerously {
            if *IS_STDOUT_TERMINAL {
                println!("{prompt}");
                let answer = Text::new("[1] Run, [2] Run & Retrieve, [3] Skip:")
                    .with_default("2")
                    .with_validator(|input: &str| match matches!(input, "1" | "2" | "3") {
                        true => Ok(Validation::Valid),
                        false => Ok(Validation::Invalid(
                            "Invalid input, please select 1, 2 or 3".into(),
                        )),
                    })
                    .prompt()?;
                match answer.as_str() {
                    "1" => {
                        let exit_code = run_command(&cmd_name, &cmd_args, Some(envs))?;
                        if exit_code != 0 {
                            bail!("Exit {exit_code}");
                        }
                        Value::Null
                    }
                    "2" => run_and_retrieve(&cmd_name, &cmd_args, envs)?,
                    _ => Value::Null,
                }
            } else {
                println!("Skipped {prompt}");
                Value::Null
            }
        } else {
            println!("{}", dimmed_text(&prompt));
            run_and_retrieve(&cmd_name, &cmd_args, envs)?
        };

        Ok(output)
    }
}

fn run_and_retrieve(
    cmd_name: &str,
    cmd_args: &[String],
    envs: HashMap<String, String>,
) -> Result<Value> {
    let (success, stdout, stderr) = run_command_with_output(cmd_name, cmd_args, Some(envs))?;

    if success {
        if !stderr.is_empty() {
            eprintln!("{}", warning_text(&stderr));
        }
        let value = if !stdout.is_empty() {
            serde_json::from_str(&stdout)
                .ok()
                .unwrap_or_else(|| json!({"output": stdout}))
        } else {
            Value::Null
        };
        Ok(value)
    } else {
        let err = if stderr.is_empty() {
            if stdout.is_empty() {
                "Something wrong"
            } else {
                &stdout
            }
        } else {
            &stderr
        };
        bail!("{err}");
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
