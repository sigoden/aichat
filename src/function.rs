use crate::{
    config::{Config, GlobalConfig},
    utils::{
        dimmed_text, get_env_bool, indent_text, run_command, run_command_with_output, warning_text,
        IS_STDOUT_TERMINAL,
    },
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

pub const FUNCTION_ALL_MATCHER: &str = ".*";
pub type ToolResults = (Vec<ToolCallResult>, String);

pub fn eval_tool_calls(
    config: &GlobalConfig,
    mut calls: Vec<ToolCall>,
) -> Result<Vec<ToolCallResult>> {
    let mut output = vec![];
    if calls.is_empty() {
        return Ok(output);
    }
    calls = ToolCall::dedup(calls);
    for call in calls {
        let result = call.eval(config)?;
        output.push(ToolCallResult::new(call, result));
    }
    Ok(output)
}

pub fn need_send_call_results(arr: &[ToolCallResult]) -> bool {
    arr.iter().any(|v| !v.output.is_null())
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ToolCallResult {
    pub call: ToolCall,
    pub output: Value,
}

impl ToolCallResult {
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

        let func_names = declarations.iter().map(|v| v.name.clone()).collect();

        Ok(Self {
            names: func_names,
            declarations,
        })
    }

    pub fn select(&self, matcher: &str) -> Option<Vec<FunctionDeclaration>> {
        let regex = Regex::new(&format!("^({matcher})$")).ok()?;
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

#[derive(Debug, Clone, Deserialize)]
pub struct FunctionConfig {
    pub enable: bool,
    pub declarations_file: String,
    pub functions_dir: String,
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
        let (call_name, cmd_name, mut cmd_args) = match &config.read().bot {
            Some(bot) => {
                if !bot.functions().contains(&function_name) {
                    bail!(
                        "Unexpected call: {} {function_name} {}",
                        bot.name(),
                        self.arguments
                    );
                }
                (
                    format!("{}:{}", bot.name(), function_name),
                    bot.name().to_string(),
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

        let output = if self.is_execute() {
            if *IS_STDOUT_TERMINAL {
                println!("{prompt}");
                let answer = Text::new("[1] Run, [2] Run & Retrieve, [3] Skip:")
                    .with_default("1")
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
                    "2" => run_and_retrieve(&cmd_name, &cmd_args, envs, &prompt)?,
                    _ => Value::Null,
                }
            } else {
                println!("Skipped {prompt}");
                Value::Null
            }
        } else {
            println!("{}", dimmed_text(&prompt));
            run_and_retrieve(&cmd_name, &cmd_args, envs, &prompt)?
        };

        Ok(output)
    }

    pub fn is_execute(&self) -> bool {
        if get_env_bool("function_auto_execute") {
            false
        } else {
            self.name.starts_with("may_") || self.name.contains("__may_")
        }
    }
}

fn run_and_retrieve(
    cmd_name: &str,
    cmd_args: &[String],
    envs: HashMap<String, String>,
    prompt: &str,
) -> Result<Value> {
    let (success, stdout, stderr) = run_command_with_output(cmd_name, cmd_args, Some(envs))?;

    if success {
        if !stderr.is_empty() {
            eprintln!(
                "{}",
                warning_text(&format!("{prompt}:\n{}", indent_text(&stderr, 4)))
            );
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
        bail!("{}", &format!("{prompt}:\n{}", indent_text(err, 4)));
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
