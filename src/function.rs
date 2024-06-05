use crate::{
    config::GlobalConfig,
    utils::{
        dimmed_text, get_env_bool, indent_text, run_command, run_command_with_output, warning_text,
        IS_STDOUT_TERMINAL,
    },
};

use anyhow::{anyhow, bail, Context, Result};
use fancy_regex::Regex;
use indexmap::{IndexMap, IndexSet};
use inquire::{validator::Validation, Text};
use lazy_static::lazy_static;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::{HashMap, HashSet},
    fs,
    path::Path,
    sync::mpsc::channel,
};
use threadpool::ThreadPool;

const BIN_DIR_NAME: &str = "bin";
const DECLARATIONS_FILE_PATH: &str = "functions.json";

lazy_static! {
    static ref THREAD_POOL: ThreadPool = ThreadPool::new(num_cpus::get());
}

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
    let parallel = calls.len() > 1 && calls.iter().all(|v| !v.is_execute());
    if parallel {
        let (tx, rx) = channel();
        let calls_len = calls.len();
        for (index, call) in calls.into_iter().enumerate() {
            let tx = tx.clone();
            let config = config.clone();
            THREAD_POOL.execute(move || {
                let result = call.eval(&config);
                let _ = tx.send((index, call, result));
            });
        }
        let mut list: Vec<(usize, ToolCall, Result<Value>)> = rx.iter().take(calls_len).collect();
        list.sort_by_key(|v| v.0);
        for (_, call, result) in list {
            output.push(ToolCallResult::new(call, result?));
        }
    } else {
        for call in calls {
            let result = call.eval(config)?;
            output.push(ToolCallResult::new(call, result));
        }
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
pub struct Function {
    names: IndexSet<String>,
    declarations: Vec<FunctionDeclaration>,
    #[cfg(windows)]
    bin_dir: std::path::PathBuf,
    env_path: Option<String>,
}

impl Function {
    pub fn init(functions_dir: &Path) -> Result<Self> {
        let bin_dir = functions_dir.join(BIN_DIR_NAME);
        let env_path = if bin_dir.exists() {
            prepend_env_path(&bin_dir).ok()
        } else {
            None
        };

        let declarations_file = functions_dir.join(DECLARATIONS_FILE_PATH);

        let declarations: Vec<FunctionDeclaration> = if declarations_file.exists() {
            let ctx = || {
                format!(
                    "Failed to load function declarations at {}",
                    declarations_file.display()
                )
            };
            let content = fs::read_to_string(&declarations_file).with_context(ctx)?;
            serde_json::from_str(&content).with_context(ctx)?
        } else {
            vec![]
        };

        let func_names = declarations.iter().map(|v| v.name.clone()).collect();

        Ok(Self {
            names: func_names,
            declarations,
            #[cfg(windows)]
            bin_dir,
            env_path,
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
        let name = self.name.clone();
        if !config.read().function.names.contains(&name) {
            bail!("Unexpected call: {name} {}", self.arguments);
        }
        let arguments = if self.arguments.is_object() {
            self.arguments.clone()
        } else if let Some(arguments) = self.arguments.as_str() {
            let args: Value = serde_json::from_str(arguments)
                .map_err(|_| anyhow!("The {name} call has invalid arguments: {arguments}"))?;
            args
        } else {
            bail!("The {name} call has invalid arguments: {}", self.arguments);
        };

        let arguments = arguments.to_string();
        let prompt = format!("Call {name} '{arguments}'",);

        let mut envs = HashMap::new();
        if let Some(env_path) = config.read().function.env_path.clone() {
            envs.insert("PATH".into(), env_path);
        };
        #[cfg(windows)]
        let name = polyfill_cmd_name(&name, &config.read().function.bin_dir);

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
                        let exit_code = run_command(&name, &[arguments], Some(envs))?;
                        if exit_code != 0 {
                            bail!("Exit {exit_code}");
                        }
                        Value::Null
                    }
                    "2" => run_and_retrieve(&name, &arguments, envs, &prompt)?,
                    _ => Value::Null,
                }
            } else {
                println!("Skipped {prompt}");
                Value::Null
            }
        } else {
            println!("{}", dimmed_text(&prompt));
            run_and_retrieve(&name, &arguments, envs, &prompt)?
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
    name: &str,
    arguments: &str,
    envs: HashMap<String, String>,
    prompt: &str,
) -> Result<Value> {
    let (success, stdout, stderr) = run_command_with_output(name, &[arguments], Some(envs))?;

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
fn polyfill_cmd_name(name: &str, bin_dir: &std::path::Path) -> String {
    let mut name = name.to_string();
    if let Ok(exts) = std::env::var("PATHEXT") {
        if let Some(cmd_path) = exts
            .split(';')
            .map(|ext| bin_dir.join(format!("{}{}", name, ext)))
            .find(|path| path.exists())
        {
            name = cmd_path.display().to_string();
        }
    }
    name
}
