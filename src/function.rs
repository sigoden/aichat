use crate::{config::GlobalConfig, utils::exec_command};

use anyhow::{anyhow, bail, Context, Result};
use fancy_regex::Regex;
use indexmap::{IndexMap, IndexSet};
use inquire::Confirm;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{collections::HashMap, fs, path::Path};

const BIN_DIR_NAME: &str = "bin";
const DECLARATIONS_FILE_PATH: &str = "functions.json";

pub fn run_tool_calls(config: &GlobalConfig, calls: &[ToolCall]) -> Result<()> {
    for call in calls {
        call.run(config)?;
    }
    Ok(())
}

#[derive(Debug, Clone, Default)]
pub struct Function {
    names: IndexSet<String>,
    declarations: Vec<FunctionDeclaration>,
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
            env_path,
        })
    }

    pub fn filtered_declarations(&self, filters: &[String]) -> Vec<FunctionDeclaration> {
        if filters.is_empty() {
            vec![]
        } else if filters.len() == 1 && filters[0] == "*" {
            self.declarations.clone()
        } else if let Ok(re) = Regex::new(&filters.join("|")) {
            self.declarations
                .iter()
                .filter(|v| re.is_match(&v.name).unwrap_or_default())
                .cloned()
                .collect()
        } else {
            vec![]
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

#[derive(Debug, Clone, Default)]
pub struct ToolCall {
    pub name: String,
    pub args: Value,
}

impl ToolCall {
    pub fn new(name: String, args: Value) -> Self {
        Self { name, args }
    }

    pub fn run(&self, config: &GlobalConfig) -> Result<()> {
        let name = &self.name;
        if !config.read().function.names.contains(name) {
            bail!("Invalid call: {name} {}", self.args);
        }
        let args = if self.args.is_object() {
            self.args.clone()
        } else if let Some(args) = self.args.as_str() {
            let args: Value =
                serde_json::from_str(args).map_err(|_| anyhow!("Invalid call args: {args}"))?;
            args
        } else {
            bail!("Invalid call args: {}", self.args);
        };
        let args = convert_args(&args);

        let prompt_text = format!(
            "call {} {}",
            name,
            args.iter()
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

        let ans = Confirm::new(&prompt_text).with_default(true).prompt()?;
        if ans {
            exec_command(name, &args, envs)?;
        }

        Ok(())
    }
}

fn convert_args(args: &Value) -> Vec<String> {
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
            convert_args(&args),
            vec!["--foo", "--bar", "val", "--baz", "v1", "--baz", "v2"]
        );
    }
}
