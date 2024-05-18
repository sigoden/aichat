use crate::{
    config::GlobalConfig,
    utils::{dimmed_text, error_text, exec_command, spawn_command},
};

use anyhow::{anyhow, bail, Context, Result};
use fancy_regex::Regex;
use indexmap::{IndexMap, IndexSet};
use inquire::Confirm;
use is_terminal::IsTerminal;
use lazy_static::lazy_static;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{collections::HashMap, fs, io::stdout, path::Path, sync::mpsc::channel};
use threadpool::ThreadPool;

const BIN_DIR_NAME: &str = "bin";
const DECLARATIONS_FILE_PATH: &str = "functions.json";

lazy_static! {
    static ref THREAD_POOL: ThreadPool = ThreadPool::new(num_cpus::get());
}

pub type ToolResults = (Vec<ToolCallResult>, String);

pub fn run_tool_calls(config: &GlobalConfig, calls: Vec<ToolCall>) -> Result<Vec<ToolCallResult>> {
    let mut output = vec![];
    if calls.is_empty() {
        return Ok(output);
    }
    let parallel = calls.len() > 1 && calls.iter().all(|v| !v.is_execute());
    if parallel {
        let (tx, rx) = channel();
        let calls_len = calls.len();
        for (index, call) in calls.into_iter().enumerate() {
            let tx = tx.clone();
            let config = config.clone();
            THREAD_POOL.execute(move || {
                let result = call.run(&config);
                let _ = tx.send((index, call, result));
            });
        }
        let mut list: Vec<(usize, ToolCall, Result<Option<Value>>)> =
            rx.iter().take(calls_len).collect();
        list.sort_by_key(|v| v.0);
        for (_, call, result) in list {
            let result = result?;
            if let Some(result) = result {
                output.push(ToolCallResult::new(call, result));
            }
        }
    } else {
        for call in calls {
            if let Some(result) = call.run(config)? {
                output.push(ToolCallResult::new(call, result));
            }
        }
    }
    Ok(output)
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

    pub fn filtered_declarations(&self, filter: Option<&str>) -> Option<Vec<FunctionDeclaration>> {
        let filter = filter?;
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
    pub fn new(name: String, arguments: Value, id: Option<String>) -> Self {
        Self {
            name,
            arguments,
            id,
        }
    }

    pub fn run(&self, config: &GlobalConfig) -> Result<Option<Value>> {
        let name = self.name.clone();
        if !config.read().function.names.contains(&name) {
            bail!("Unexpected call: {name} {}", self.arguments);
        }
        let arguments = if self.arguments.is_object() {
            self.arguments.clone()
        } else if let Some(arguments) = self.arguments.as_str() {
            let args: Value = serde_json::from_str(arguments)
                .map_err(|_| anyhow!("Invalid call arguments: {arguments}"))?;
            args
        } else {
            bail!("Invalid call arguments: {}", self.arguments);
        };
        let arguments = convert_arguments(&arguments);

        let prompt_text = format!(
            "Call {} {}",
            name,
            arguments
                .iter()
                .map(|v| shell_words::quote(v).to_string())
                .collect::<Vec<String>>()
                .join(" ")
        );

        let envs = if let Some(env_path) = config.read().function.env_path.clone() {
            let mut envs = HashMap::new();
            envs.insert("PATH".into(), env_path);
            Some(envs)
        } else {
            None
        };
        let output = if self.is_execute() {
            let proceed = if stdout().is_terminal() {
                Confirm::new(&prompt_text).with_default(true).prompt()?
            } else {
                println!("{}", dimmed_text(&prompt_text));
                true
            };
            if proceed {
                #[cfg(windows)]
                let name = polyfill_cmd_name(&name, &config.read().function.bin_dir);
                spawn_command(&name, &arguments, envs)?;
            }
            None
        } else {
            println!("{}", dimmed_text(&prompt_text));
            #[cfg(windows)]
            let name = polyfill_cmd_name(&name, &config.read().function.bin_dir);
            let (success, stdout, stderr) = exec_command(&name, &arguments, envs)?;
            if stderr.is_empty() {
                eprintln!("{}", error_text(&stderr));
            }
            if success && !stdout.is_empty() {
                serde_json::from_str(&stdout)
                    .ok()
                    .or_else(|| Some(json!({"output": stdout})))
            } else {
                None
            }
        };

        Ok(output)
    }

    pub fn is_execute(&self) -> bool {
        self.name.starts_with("execute_") || self.name.contains("__execute_")
    }
}

fn convert_arguments(args: &Value) -> Vec<String> {
    let mut options: Vec<String> = Vec::new();

    if let Value::Object(map) = args {
        for (key, value) in map {
            let key = key.replace('_', "-");
            match value {
                Value::Bool(true) => {
                    options.push(format!("--{key}"));
                }
                Value::String(s) => {
                    options.push(format!("--{key}"));
                    options.push(s.to_string());
                }
                Value::Array(arr) => {
                    for item in arr {
                        if let Value::String(s) = item {
                            options.push(format!("--{key}"));
                            options.push(s.to_string());
                        }
                    }
                }
                _ => {} // Ignore other types
            }
        }
    }
    options
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

#[cfg(test)]
mod tests {

    use super::*;

    #[test]
    fn test_convert_args() {
        let args = serde_json::json!({
          "foo": true,
          "bar": "val",
          "baz": ["v1", "v2"]
        });
        assert_eq!(
            convert_arguments(&args),
            vec!["--foo", "--bar", "val", "--baz", "v1", "--baz", "v2"]
        );
    }
}
